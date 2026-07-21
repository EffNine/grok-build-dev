#!/bin/bash
#
# Grok Build (BYOK fork) installer — installs from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/effnine/grok-build-dev/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/effnine/grok-build-dev/main/install.sh | bash -s 0.2.106
#
# Env:
#   GROK_REPO      GitHub owner/repo (default: effnine/grok-build-dev)
#   GROK_BIN_DIR   Install bin dir (default: ~/.grok/bin)
#   GROK_CHANNEL   stable|alpha (default: stable); alpha includes pre-releases
#
# Artifact naming matches the in-app updater: grok-{version}-{os}-{arch}
# e.g. grok-0.2.106-macos-aarch64

set -e

TARGET="$1"
REPO="${GROK_REPO:-effnine/grok-build-dev}"
CHANNEL="${GROK_CHANNEL:-stable}"

if [[ -n "$TARGET" ]] && [[ ! "$TARGET" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9._]+)?$ ]]; then
    echo "Invalid version format: $TARGET (expected X.Y.Z or X.Y.Z-suffix)" >&2
    exit 1
fi

DOWNLOADER=""
if command -v curl >/dev/null 2>&1; then
    DOWNLOADER="curl"
elif command -v wget >/dev/null 2>&1; then
    DOWNLOADER="wget"
else
    echo "Either curl or wget is required but neither is installed" >&2
    exit 1
fi

download_file() {
    local url="$1" output="$2"
    if [ "$DOWNLOADER" = "curl" ]; then
        if [ -n "$output" ]; then
            curl -fsSL -o "$output" "$url"
        else
            curl -fsSL "$url"
        fi
    else
        if [ -n "$output" ]; then
            wget -q -O "$output" "$url"
        else
            wget -q -O - "$url"
        fi
    fi
}

# Parallel byte-range download. Falls back to single-connection download_file
# whenever HEAD lacks Content-Length, the file is small (<16 MiB), curl is
# unavailable, or any chunk fetch / concat fails.
download_file_parallel() {
    local url="$1" output="$2"
    if [ "$DOWNLOADER" != "curl" ]; then
        download_file "$url" "$output"
        return
    fi
    local size
    size=$(curl -fsSL --head "$url" 2>/dev/null | awk -F'[: \r\n]+' 'tolower($1)=="content-length"{print $2; exit}')
    if [ -z "$size" ] || ! [ "$size" -ge 16777216 ] 2>/dev/null; then
        download_file "$url" "$output"
        return
    fi
    local n=8
    local chunk_size=$(( (size + n - 1) / n ))
    local tmpdir
    tmpdir=$(mktemp -d 2>/dev/null) || { download_file "$url" "$output"; return; }
    local pids=() i start end
    for i in $(seq 0 $((n - 1))); do
        start=$((i * chunk_size))
        end=$((start + chunk_size - 1))
        [ $end -ge $size ] && end=$((size - 1))
        curl -fsSL -r "${start}-${end}" -o "${tmpdir}/$(printf 'chunk.%03d' "$i")" "$url" &
        pids+=($!)
    done
    local all_ok=true pid
    for pid in "${pids[@]}"; do
        wait "$pid" || all_ok=false
    done
    if [ "$all_ok" = true ] && cat "${tmpdir}"/chunk.* > "$output" 2>/dev/null; then
        rm -rf "$tmpdir"
        return 0
    fi
    rm -rf "$tmpdir"
    download_file "$url" "$output"
}

is_not_found() {
    local url="$1" code
    if [ "$DOWNLOADER" = "curl" ]; then
        code=$(curl -o /dev/null -sSL -w '%{http_code}' --head "$url" 2>/dev/null) || true
    else
        code=$(wget --server-response --spider "$url" 2>&1 | awk '/HTTP\//{print $2}' | tail -1) || true
    fi
    [ "$code" = "404" ]
}

# Extract a top-level JSON string field (tag_name, browser_download_url, …).
json_get() {
    local json="$1" field="$2"
    printf '%s' "$json" | sed -n -E 's/.*"'"$field"'"[[:space:]]*:[[:space:]]*"(([^"\\]|\\.)*)".*/\1/p' | head -1 \
        | sed -e 's/\\"/"/g' -e 's/\\n/\'$'\n''/g' -e 's/\\t/\'$'\t''/g' -e 's/\\\\/\\/g'
}

case "$(uname -s)" in
    Darwin) os="macos" ;;
    Linux)  os="linux" ;;
    MINGW* | MSYS* | CYGWIN*) os="windows" ;;
    *)      echo "Unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
    x86_64|amd64|AMD64) arch="x86_64" ;;
    arm64|aarch64|ARM64) arch="aarch64" ;;
    *)                    echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

platform="${os}-${arch}"
DOWNLOAD_DIR="$HOME/.grok/downloads"
BIN_DIR="${GROK_BIN_DIR:-$HOME/.grok/bin}"
mkdir -p "$DOWNLOAD_DIR" "$BIN_DIR"

API_BASE="https://api.github.com/repos/${REPO}"
RELEASE_BASE="https://github.com/${REPO}/releases/download"

if [ -n "$TARGET" ]; then
    version="$TARGET"
    tag="v${version}"
    echo "Installing Grok $version ($platform) from ${REPO}..." >&2
else
    echo "Fetching latest ${CHANNEL} release from ${REPO}..." >&2
    if [ "$CHANNEL" = "alpha" ]; then
        # Latest published release including pre-releases.
        release_json=$(download_file "${API_BASE}/releases?per_page=1" 2>/dev/null) || true
        # API returns an array; wrap extraction by taking the first object.
        release_json=$(printf '%s' "$release_json" | tr '\n' ' ' | sed -n 's/^[[:space:]]*\[[[:space:]]*\({.*}\)[[:space:]]*\][[:space:]]*$/\1/p')
    else
        release_json=$(download_file "${API_BASE}/releases/latest" 2>/dev/null) || true
    fi
    if [ -z "$release_json" ]; then
        echo "Error: failed to fetch release metadata from ${API_BASE}" >&2
        echo "Hint: create a GitHub Release (tag vX.Y.Z) with platform binaries, or pass a version explicitly." >&2
        exit 1
    fi
    tag=$(json_get "$release_json" "tag_name")
    if [ -z "$tag" ]; then
        echo "Error: no releases found for ${REPO}" >&2
        exit 1
    fi
    version="${tag#v}"
    echo "Installing Grok $version ($platform) from ${REPO}..." >&2
fi

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9._]+)?$ ]]; then
    echo "Invalid version format: $version (expected X.Y.Z or X.Y.Z-suffix)" >&2
    exit 1
fi

artifact_name="grok-${version}-${platform}"
if [ "$os" = "windows" ]; then
    artifact_name="${artifact_name}.exe"
fi
artifact_url="${RELEASE_BASE}/v${version}/${artifact_name}"

binary_path="$DOWNLOAD_DIR/grok-$platform"
if [ "$os" = "windows" ]; then
    binary_path="${binary_path}.exe"
fi
binary_tmp="${binary_path}.tmp.$$"
rm -f "$binary_tmp" 2>/dev/null || true

echo "  Downloading ${artifact_name}..." >&2
if ! download_file_parallel "$artifact_url" "$binary_tmp"; then
    rm -f "$binary_tmp"
    if is_not_found "$artifact_url"; then
        echo "Error: no binary for $platform in release v${version}." >&2
        echo "  Expected asset: ${artifact_name}" >&2
        echo "  Release URL: https://github.com/${REPO}/releases/tag/v${version}" >&2
    else
        echo "Error: binary download failed from ${artifact_url}" >&2
    fi
    exit 1
fi

if [ "$os" = "windows" ]; then
    mv -f "$binary_tmp" "$binary_path"
    for bin_name in grok.exe agent.exe; do
        rm -f "$BIN_DIR/$bin_name.old" 2>/dev/null || true
        if ! cp -f "$binary_path" "$BIN_DIR/$bin_name" 2>/dev/null; then
            mv -f "$BIN_DIR/$bin_name" "$BIN_DIR/$bin_name.old" 2>/dev/null || true
            if ! cp -f "$binary_path" "$BIN_DIR/$bin_name" 2>/dev/null; then
                mv -f "$BIN_DIR/$bin_name.old" "$BIN_DIR/$bin_name" 2>/dev/null || true
                echo "Error: failed to install $bin_name" >&2
                exit 1
            fi
        fi
    done
    echo "  Binary installed to $BIN_DIR/grok.exe and $BIN_DIR/agent.exe." >&2
else
    chmod +x "$binary_tmp"
    if ! "$binary_tmp" --version </dev/null >/dev/null 2>&1; then
        echo "Error: downloaded grok failed to run; keeping the existing install." >&2
        rm -f "$binary_tmp"
        exit 1
    fi
    # Prefer the versioned name used by `grok update` (gh-release installer).
    versioned_path="$DOWNLOAD_DIR/grok-${version}-${platform}"
    mv -f "$binary_tmp" "$versioned_path"
    binary_path="$versioned_path"
    if [ "$(dirname "$BIN_DIR")" = "$(dirname "$DOWNLOAD_DIR")" ]; then
        link_target="../$(basename "$DOWNLOAD_DIR")/$(basename "$binary_path")"
    else
        link_target="$binary_path"
    fi
    ln -sf "$link_target" "$BIN_DIR/grok"
    ln -sf "$link_target" "$BIN_DIR/agent"
    echo "  Binary linked to $BIN_DIR/grok and $BIN_DIR/agent." >&2
fi

# Generate shell completions (best-effort)
mkdir -p "$HOME/.grok/completions/bash" "$HOME/.grok/completions/zsh"
"$BIN_DIR/grok" completions bash > "$HOME/.grok/completions/bash/grok.bash" 2>/dev/null || true
"$BIN_DIR/grok" completions zsh  > "$HOME/.grok/completions/zsh/_grok"     2>/dev/null || true
if mkdir -p "$HOME/.config/fish/completions" 2>/dev/null; then
    "$BIN_DIR/grok" completions fish > "$HOME/.config/fish/completions/grok.fish" 2>/dev/null || true
fi

# Persist installer source so `grok update` uses GitHub Releases.
CONFIG_FILE="$HOME/.grok/config.toml"
CLI_BLOCK='installer = "gh-release"'
if [ ! -f "$CONFIG_FILE" ]; then
    printf '[cli]\n%s\n' "$CLI_BLOCK" > "$CONFIG_FILE"
elif grep -q '^\[cli\]' "$CONFIG_FILE"; then
    tmp="$CONFIG_FILE.tmp.$$"
    awk -v block="$CLI_BLOCK" '
        /^\[cli\][[:space:]]*(#.*)?$/ { print; printf "%s\n", block; in_cli=1; next }
        /^\[.*\][[:space:]]*(#.*)?$/  { in_cli=0 }
        in_cli && /^[[:space:]]*(installer|channel)[[:space:]]*=/ { next }
        { print }
    ' "$CONFIG_FILE" > "$tmp" && mv "$tmp" "$CONFIG_FILE"
else
    printf '\n[cli]\n%s\n' "$CLI_BLOCK" >> "$CONFIG_FILE"
fi

if [ "$os" = "windows" ]; then
    echo "Grok $version installed to $BIN_DIR/grok.exe" >&2
else
    echo "Grok $version installed to $BIN_DIR/grok" >&2
fi

# --- Ensure grok is on PATH ---

path_has_dir() {
    case ":$PATH:" in *":$1:"*) return 0 ;; *) return 1 ;; esac
}

SYMLINK_CREATED=""
if [ "$os" != "windows" ] && ! path_has_dir "$BIN_DIR"; then
    for candidate in "$HOME/.local/bin" "/usr/local/bin"; do
        if path_has_dir "$candidate" && [ -d "$candidate" ] && [ -w "$candidate" ]; then
            ln -sf "$BIN_DIR/grok" "$candidate/grok"
            ln -sf "$BIN_DIR/agent" "$candidate/agent"
            SYMLINK_CREATED="$candidate"
            echo "  Symlinked $candidate/grok -> $BIN_DIR/grok" >&2
            echo "  Symlinked $candidate/agent -> $BIN_DIR/agent" >&2
            break
        fi
    done
fi

user_shell="$(basename "${SHELL:-}")"
config_file=""

case "$user_shell" in
    bash) config_file="$HOME/.bashrc" ;;
    zsh)  config_file="$HOME/.zshrc" ;;
    fish) config_file="$HOME/.config/fish/config.fish" ;;
esac

if [ -n "$config_file" ]; then
    mkdir -p "$(dirname "$config_file")"

    if [ -e "$config_file" ] || [ -L "$config_file" ]; then
        _cf="$config_file"
        _depth=0
        while [ -L "$_cf" ] && [ "$_depth" -lt 40 ]; do
            _link="$(readlink "$_cf")" || break
            case "$_link" in
                /*) _cf="$_link" ;;
                *)  _cf="$(cd "$(dirname "$_cf")" && pwd -P)/$_link" ;;
            esac
            _depth=$((_depth + 1))
        done
        if [ ! -L "$_cf" ]; then
            config_file="$(cd "$(dirname "$_cf")" && pwd -P)/$(basename "$_cf")"
        fi
        unset _cf _link _depth
    fi

    if [ "$user_shell" = "fish" ]; then
        new_block='# >>> grok installer >>>
fish_add_path $HOME/.grok/bin
# <<< grok installer <<<'
    elif [ "$user_shell" = "zsh" ]; then
        new_block='# >>> grok installer >>>
export PATH="$HOME/.grok/bin:$PATH"
fpath=(~/.grok/completions/zsh $fpath)
autoload -Uz compinit && compinit -C
# <<< grok installer <<<'
    else
        new_block='# >>> grok installer >>>
export PATH="$HOME/.grok/bin:$PATH"
[[ -r "$HOME/.grok/completions/bash/grok.bash" ]] && source "$HOME/.grok/completions/bash/grok.bash"
# <<< grok installer <<<'
    fi

    if grep -qs "grok installer" "$config_file" 2>/dev/null; then
        tmp="$config_file.tmp.$$"
        awk '
            /# >>> grok installer >>>/ { skip=1; next }
            /# <<< grok installer <<</ { skip=0; next }
            !skip { print }
        ' "$config_file" > "$tmp" && mv "$tmp" "$config_file"
    else
        [ -f "$config_file" ] && cp "$config_file" "$config_file.bak.$(date +%s)"

        if [ "$user_shell" = "bash" ] && [ "$(uname -s)" = "Darwin" ]; then
            if [ -f "$HOME/.bash_profile" ] && ! grep -qs "source ~/.bashrc" "$HOME/.bash_profile"; then
                printf '\n[[ -r ~/.bashrc ]] && source ~/.bashrc\n' >> "$HOME/.bash_profile"
            fi
        fi
    fi

    printf '\n%s\n' "$new_block" >> "$config_file"
    echo "  Updated $BIN_DIR in PATH in $config_file." >&2
fi

echo "" >&2
echo "This is the BYOK fork — set a provider key before first use:" >&2
echo '  export XAI_API_KEY="sk-..."' >&2
echo '  export GROK_MODELS_BASE_URL="https://api.openai.com/v1"' >&2
echo "  # or run /byok inside the TUI" >&2
echo "" >&2

if path_has_dir "$BIN_DIR" || [ -n "$SYMLINK_CREATED" ]; then
    echo "Run 'grok' or 'agent' to get started!" >&2
elif [ -n "$config_file" ]; then
    echo "Restart your terminal, then run 'grok' or 'agent' to get started!" >&2
else
    echo "Add $BIN_DIR to your PATH, then run 'grok' or 'agent' to get started:" >&2
    echo '  export PATH="$HOME/.grok/bin:$PATH"' >&2
fi

if [ "$os" = "windows" ]; then
    echo "To use grok from cmd.exe or PowerShell, add %USERPROFILE%\\.grok\\bin to your PATH." >&2
fi
