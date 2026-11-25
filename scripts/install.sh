#!/bin/sh
# OpenMesh installer and uninstaller.
#
# Installs the openmesh-bridge daemon, the openmesh-cli tool and (optionally) the
# web control panel from a GitHub release, verifies the SHA-256 checksums, and can
# install a systemd user service for backbone mode.
#
#   curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh
#   ./install.sh --version v0.2.0
#   ./install.sh --service
#   ./install.sh --uninstall
#
# POSIX sh; no root needed unless --system is used. See --help and docs/INSTALL.md.

set -eu

# --------------------------------------------------------------------------- constants
OM_REPO_DEFAULT="martintondel02/openmesh"
OM_API_DEFAULT="https://api.github.com/repos/${OM_REPO_DEFAULT}/releases/latest"

# --------------------------------------------------------------------------- defaults
om_repo=${OPENMESH_REPO:-$OM_REPO_DEFAULT}
om_version=${OPENMESH_VERSION:-}
om_base_url=${OPENMESH_BASE_URL:-}
om_system=0
om_service=0
om_dry_run=0
om_uninstall=0
om_yes=0
om_install_web=1
om_install_docs=1
om_bin_dir_opt=""
om_data_dir_opt=""
om_platform_override=${OPENMESH_PLATFORM:-}
om_tmp=""

om_os=""
om_arch=""
om_platform=""
om_exe=""
om_tag=""
om_base=""
om_asset=""
om_web_asset=""
om_sudo=""
om_system_root=""

# Resolved install locations (filled by om_resolve_paths).
om_bin_dir=""
om_web_dir=""
om_doc_dir=""
om_data_dir=""
om_config_dir=""
om_service_dir=""
om_unit_name="openmesh-backbone.service"

# --------------------------------------------------------------------------- output
om_say() { printf '%s\n' "$*"; }
om_info() { printf '==> %s\n' "$*" >&2; }
om_warn() { printf 'warning: %s\n' "$*" >&2; }
om_die() { printf 'error: %s\n' "$*" >&2; exit 1; }

om_cleanup() {
    if [ -n "$om_tmp" ] && [ -d "$om_tmp" ]; then
        rm -rf "$om_tmp"
    fi
}
trap 'om_cleanup' 0
trap 'om_cleanup; exit 130' 1 2 15

# --------------------------------------------------------------------------- help
om_usage() {
    cat <<'EOF'
OpenMesh installer

Usage:
  install.sh [OPTIONS]

Installs openmesh-bridge and openmesh-cli from the latest GitHub release (or a pinned
version), verifies SHA-256 checksums, and optionally installs the web control panel and a
systemd user service for backbone mode. No root privileges are required by default.

Options:
  --version VERSION     Install a specific release tag (v0.2.0 or 0.2.0).
  --system              Install to /usr/local/bin and /usr/share/openmesh (uses sudo).
  --service             Install and enable a systemd user unit for backbone mode.
                        Cannot be combined with --system.
  --bin-dir DIR         Install binaries into DIR instead of ~/.local/bin.
  --data-dir DIR        Data directory for the installed service (default ~/.openmesh).
  --no-web              Do not install the web control panel bundle.
  --dry-run             Print the plan; download and change nothing.
  --uninstall           Remove installed binaries, web bundle, docs and the unit
                        written by this installer. Config and data are kept.
  -y, --yes             Do not ask for confirmation (implies non-interactive).
  -h, --help            Show this help.
  -V, --version-info    Show the installer version.

Environment:
  OPENMESH_BASE_URL     Download base URL or local directory for release assets
                        (used by tests/mirrors; assets and SHA256SUMS.txt live there,
                        and VERSION supplies the tag when --version is omitted).
  OPENMESH_VERSION      Same as --version.
  OPENMESH_REPO         GitHub repository (default martintondel02/openmesh).
  OPENMESH_API_URL      Releases API endpoint for "latest" resolution
                        (default https://api.github.com/repos/.../releases/latest).
  OPENMESH_PLATFORM     Override platform detection: linux-x86_64, macos-arm64,
                        windows-x86_64 (advanced/testing).
  OPENMESH_BIN_DIR, OPENMESH_WEB_DIR, OPENMESH_DOC_DIR, OPENMESH_DATA_DIR,
  OPENMESH_CONFIG_DIR, OPENMESH_SYSTEMD_DIR
                        Override install locations (advanced/testing).
  OPENMESH_PREFIX       Install into PREFIX as a filesystem root (testing/packaging):
                        disables sudo, puts binaries in PREFIX/usr/local/bin, etc.
  OPENMESH_SKIP_SYSTEMCTL=1
                        Write the unit but do not call systemctl (testing/containers).
  GITHUB_TOKEN          Optional token for the GitHub releases API rate limit.

Examples:
  curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh
  ./install.sh --version v0.2.0 --service
  ./install.sh --dry-run
  ./install.sh --uninstall

The script is idempotent; existing configuration, tokens and data are never overwritten.
EOF
}

# --------------------------------------------------------------------------- small helpers
om_have() { command -v "$1" >/dev/null 2>&1; }

om_run() {
    # Execute a command, honouring --dry-run and sudo.
    if [ "$om_dry_run" = 1 ]; then
        om_say "[dry-run] would run: $*"
        return 0
    fi
    if [ -n "$om_sudo" ]; then
        "$om_sudo" "$@"
    else
        "$@"
    fi
}

om_sd_token() {
    # Return a double-quoted systemd ExecStart argument for a path.
    printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
}

om_sha256() {
    if om_have sha256sum; then
        sha256sum "$1" | awk '{print $1}'
    elif om_have shasum; then
        shasum -a 256 "$1" | awk '{print $1}'
    elif om_have openssl; then
        openssl dgst -sha256 "$1" | sed 's/^.*= //'
    else
        om_die "no SHA-256 tool found (sha256sum, shasum or openssl); install one and retry"
    fi
}

om_lower() { printf '%s' "$1" | tr 'A-F' 'a-f'; }

om_download() {
    # om_download URL_OR_PATH DEST [AUTH_HEADER]
    om_url=$1
    om_dest=$2
    om_auth=${3:-}
    case $om_url in
        http://*|https://*)
            if om_have curl; then
                if [ -n "$om_auth" ]; then
                    curl -fsSL --retry 3 --connect-timeout 15 --max-time 300 \
                        -H "$om_auth" -o "$om_dest" "$om_url" ||
                        om_die "download failed: $om_url (check your network/proxy)"
                else
                    curl -fsSL --retry 3 --connect-timeout 15 --max-time 300 \
                        -o "$om_dest" "$om_url" ||
                        om_die "download failed: $om_url (check your network/proxy)"
                fi
            elif om_have wget; then
                if [ -n "$om_auth" ]; then
                    wget -q --header="$om_auth" -O "$om_dest" "$om_url" ||
                        om_die "download failed: $om_url (check your network/proxy)"
                else
                    wget -q -O "$om_dest" "$om_url" ||
                        om_die "download failed: $om_url (check your network/proxy)"
                fi
            else
                om_die "curl or wget is required to download releases"
            fi
            ;;
        file://*)
            om_src=${om_url#file://}
            [ -f "$om_src" ] || om_die "local release file not found: $om_src"
            cp "$om_src" "$om_dest"
            ;;
        *)
            [ -f "$om_url" ] || om_die "local release file not found: $om_url"
            cp "$om_url" "$om_dest"
            ;;
    esac
}

om_try_download() {
    # Like om_download, but returns non-zero instead of exiting; used for optional probes.
    om_url=$1
    om_dest=$2
    case $om_url in
        http://*|https://*)
            if om_have curl; then
                curl -fsSL --retry 1 --connect-timeout 10 --max-time 60 -o "$om_dest" "$om_url" 2>/dev/null
            elif om_have wget; then
                wget -q -O "$om_dest" "$om_url" 2>/dev/null
            else
                return 1
            fi
            ;;
        file://*)
            om_src=${om_url#file://}
            [ -f "$om_src" ] || return 1
            cp "$om_src" "$om_dest"
            ;;
        *)
            [ -f "$om_url" ] || return 1
            cp "$om_url" "$om_dest"
            ;;
    esac
}

om_confirm() {
    # Ask on the controlling terminal; never consume the script's own stdin (curl | sh).
    om_prompt=$1
    if [ "$om_yes" = 1 ]; then
        return 0
    fi
    if [ -r /dev/tty ]; then
        printf '%s [y/N] ' "$om_prompt" >/dev/tty
        if read -r om_answer </dev/tty; then :; else om_answer=""; fi
        case $om_answer in
            y|Y|yes|YES) return 0 ;;
            *) om_die "aborted by user" ;;
        esac
    fi
    om_die "confirmation required but no terminal is available; pass --yes"
}

# --------------------------------------------------------------------------- argument parsing
om_parse_args() {
    while [ $# -gt 0 ]; do
        case $1 in
            --version)
                [ $# -ge 2 ] || om_die "--version needs a value"
                om_version=$2
                shift 2
                ;;
            --system)
                om_system=1
                shift
                ;;
            --service)
                om_service=1
                shift
                ;;
            --bin-dir)
                [ $# -ge 2 ] || om_die "--bin-dir needs a value"
                om_bin_dir_opt=$2
                shift 2
                ;;
            --data-dir)
                [ $# -ge 2 ] || om_die "--data-dir needs a value"
                om_data_dir_opt=$2
                shift 2
                ;;
            --base-url)
                [ $# -ge 2 ] || om_die "--base-url needs a value"
                om_base_url=$2
                shift 2
                ;;
            --no-web)
                om_install_web=0
                shift
                ;;
            --dry-run)
                om_dry_run=1
                shift
                ;;
            --uninstall)
                om_uninstall=1
                shift
                ;;
            -y|--yes)
                om_yes=1
                shift
                ;;
            -h|--help)
                om_usage
                exit 0
                ;;
            -V|--version-info)
                om_say "openmesh install.sh (v0.2.0 installer)"
                exit 0
                ;;
            *)
                om_die "unknown option: $1 (try --help)"
                ;;
        esac
    done

    if [ "$om_service" = 1 ] && [ "$om_system" = 1 ]; then
        om_die "--service installs a systemd user unit and cannot be combined with --system.
       For a system-wide service use packaging/systemd/openmesh-backbone.service."
    fi
    if [ "$om_uninstall" = 1 ] && [ "$om_service" = 1 ]; then
        om_die "--uninstall already removes the unit installed by --service; do not combine them"
    fi
}

# --------------------------------------------------------------------------- platform + paths
om_detect_platform() {
    om_os=$(uname -s 2>/dev/null || printf unknown)
    om_arch=$(uname -m 2>/dev/null || printf unknown)

    if [ -n "$om_platform_override" ]; then
        case $om_platform_override in
            linux-x86_64|macos-arm64) om_platform=$om_platform_override; om_exe="" ;;
            windows-x86_64) om_platform=$om_platform_override; om_exe=".exe" ;;
            *)
                om_die "unsupported OPENMESH_PLATFORM: $om_platform_override
       supported: linux-x86_64, macos-arm64, windows-x86_64"
                ;;
        esac
        return 0
    fi

    case $om_os in
        Linux)
            case $om_arch in
                x86_64|amd64) om_platform="linux-x86_64" ;;
                aarch64|arm64)
                    om_die "no official linux-aarch64 build yet (release matrix: linux-x86_64,
       macos-arm64, windows-x86_64). Build from source with
       'cargo build --release -p openmesh-bridge -p openmesh-cli',
       or run the x86_64 Docker image on an x86_64 host."
                    ;;
                *)
                    om_die "unsupported Linux architecture: $om_arch (supported: x86_64)"
                    ;;
            esac
            ;;
        Darwin)
            case $om_arch in
                arm64) om_platform="macos-arm64" ;;
                x86_64)
                    om_die "no official build for Intel macOS (release matrix has macos-arm64
       only). Build from source with 'cargo build --release -p openmesh-bridge -p openmesh-cli'."
                    ;;
                *)
                    om_die "unsupported macOS architecture: $om_arch"
                    ;;
            esac
            ;;
        MINGW*|MSYS*|CYGWIN*)
            om_platform="windows-x86_64"
            om_exe=".exe"
            om_warn "Windows detected (Git Bash/MSYS). The release archive is a .tar.gz;
       see docs/INSTALL.md for native Windows extraction and service notes."
            ;;
        *)
            om_die "unsupported operating system: $om_os (supported: Linux, macOS, Windows via Git Bash)"
            ;;
    esac
}

om_resolve_paths() {
    om_system_root=${OPENMESH_PREFIX:-}
    if [ "$om_system" = 1 ]; then
        if [ -n "$om_system_root" ]; then
            # Filesystem-root install for tests/packaging; no sudo, no systemd.
            om_bin_dir=${OPENMESH_BIN_DIR:-${om_bin_dir_opt:-$om_system_root/usr/local/bin}}
            om_web_dir=${OPENMESH_WEB_DIR:-$om_system_root/usr/share/openmesh/web}
            om_doc_dir=${OPENMESH_DOC_DIR:-$om_system_root/usr/share/doc/openmesh}
            om_data_dir=${OPENMESH_DATA_DIR:-${om_data_dir_opt:-$om_system_root/var/lib/openmesh}}
            om_config_dir=${OPENMESH_CONFIG_DIR:-$om_system_root/etc/openmesh}
            om_service_dir=${OPENMESH_SYSTEMD_DIR:-$om_system_root/etc/systemd/system}
        else
            om_bin_dir=${OPENMESH_BIN_DIR:-${om_bin_dir_opt:-/usr/local/bin}}
            om_web_dir=${OPENMESH_WEB_DIR:-/usr/share/openmesh/web}
            om_doc_dir=${OPENMESH_DOC_DIR:-/usr/share/doc/openmesh}
            om_data_dir=${OPENMESH_DATA_DIR:-${om_data_dir_opt:-/var/lib/openmesh}}
            om_config_dir=${OPENMESH_CONFIG_DIR:-/etc/openmesh}
            om_service_dir=${OPENMESH_SYSTEMD_DIR:-/etc/systemd/system}
        fi
        if [ -z "$om_system_root" ]; then
            om_have sudo || om_die "--system needs sudo, which was not found on PATH"
            om_sudo="sudo"
        fi
    else
        [ -n "${HOME:-}" ] || om_die "HOME is not set; set HOME or use --system"
        om_xdg_data=${XDG_DATA_HOME:-$HOME/.local/share}
        om_xdg_config=${XDG_CONFIG_HOME:-$HOME/.config}
        om_bin_dir=${OPENMESH_BIN_DIR:-${om_bin_dir_opt:-$HOME/.local/bin}}
        om_web_dir=${OPENMESH_WEB_DIR:-$om_xdg_data/openmesh/web}
        om_doc_dir=${OPENMESH_DOC_DIR:-$om_xdg_data/doc/openmesh}
        # The documented deployment (README, setup payload, units) uses ~/.openmesh so that
        # config, identity, database and the panel share one predictable directory.
        om_data_dir=${OPENMESH_DATA_DIR:-${om_data_dir_opt:-$HOME/.openmesh}}
        om_config_dir=${OPENMESH_CONFIG_DIR:-$om_xdg_config/openmesh}
        om_service_dir=${OPENMESH_SYSTEMD_DIR:-$om_xdg_config/systemd/user}
        om_sudo=""
    fi
}

# --------------------------------------------------------------------------- release resolution
om_resolve_version() {
    if [ -n "$om_version" ]; then
        case $om_version in
            v*) om_tag=$om_version ;;
            *) om_tag="v$om_version" ;;
        esac
    elif [ -n "${OPENMESH_API_URL:-}" ] || [ -z "$om_base_url" ]; then
        om_info "resolving the latest OpenMesh release"
        om_api=${OPENMESH_API_URL:-$OM_API_DEFAULT}
        om_auth=""
        if [ -n "${GITHUB_TOKEN:-}" ]; then
            om_auth="Authorization: Bearer $GITHUB_TOKEN"
        fi
        om_download "$om_api" "$om_tmp/release.json" "$om_auth" ||
            om_die "could not query the releases API.
       Check your network, or pass --version vX.Y.Z / set OPENMESH_BASE_URL."
        om_tag=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
            "$om_tmp/release.json" | head -n 1)
        [ -n "$om_tag" ] ||
            om_die "the releases API response did not contain a tag_name; pass --version vX.Y.Z"
    else
        # Mirrors/tests provide a VERSION file; --version is required if it is absent.
        if om_try_download "$om_base_url/VERSION" "$om_tmp/VERSION"; then
            om_tag=$(tr -d '[:space:]' <"$om_tmp/VERSION")
        fi
        [ -n "$om_tag" ] ||
            om_die "could not determine the release version from $om_base_url/VERSION;
       pass --version vX.Y.Z"
    fi

    case $om_tag in
        v*) : ;;
        *) om_tag="v$om_tag" ;;
    esac
    om_version=$om_tag

    if [ -n "$om_base_url" ]; then
        om_base=${om_base_url%/}
    else
        om_base="https://github.com/$om_repo/releases/download/$om_tag"
    fi

    om_asset="openmesh-${om_tag}-${om_platform}.tar.gz"
    om_web_asset="openmesh-web-${om_tag}.tar.gz"
}

om_check_tools() {
    om_have tar || om_die "tar is required"
    om_have awk || om_die "awk is required"
    om_have sed || om_die "sed is required"
    if [ "$om_dry_run" = 0 ]; then
        if ! om_have curl && ! om_have wget; then
            case $om_base in
                http://*|https://*) om_die "curl or wget is required to download releases" ;;
            esac
        fi
    fi
}

# --------------------------------------------------------------------------- checksum + unpack
om_lookup_checksum() {
    om_want=$1
    awk -v want="$om_want" '
        {
            name = $2
            sub(/^\*/, "", name)
            if (name == want) { print $1; exit }
        }
    ' "$om_tmp/SHA256SUMS.txt"
}

om_verify_asset() {
    om_file=$1
    om_name=$2
    om_expected=$(om_lookup_checksum "$om_name")
    if [ -z "$om_expected" ]; then
        om_die "SHA256SUMS.txt does not list $om_name.
       Refusing to install an unverifiable download. If you are testing with a mirror,
       regenerate SHA256SUMS.txt; otherwise the release is incomplete."
    fi
    om_actual=$(om_sha256 "$om_file")
    om_expected=$(om_lower "$om_expected")
    om_actual=$(om_lower "$om_actual")
    if [ "$om_actual" != "$om_expected" ]; then
        om_die "checksum mismatch for $om_name
       expected: $om_expected
       actual:   $om_actual
       The download may be corrupt or tampered with; nothing was installed."
    fi
    om_info "sha256 verified: $om_name"
}

om_fetch_release() {
    om_info "downloading SHA256SUMS.txt"
    om_download "$om_base/SHA256SUMS.txt" "$om_tmp/SHA256SUMS.txt"
    om_info "downloading $om_asset ($om_version, $om_platform)"
    om_download "$om_base/$om_asset" "$om_tmp/$om_asset"
    om_verify_asset "$om_tmp/$om_asset" "$om_asset"
    if [ "$om_install_web" = 1 ]; then
        om_info "downloading $om_web_asset"
        om_download "$om_base/$om_web_asset" "$om_tmp/$om_web_asset"
        om_verify_asset "$om_tmp/$om_web_asset" "$om_web_asset"
    fi
}

om_unpack_binary_asset() {
    om_archive=$1
    om_top=$(tar -tzf "$om_archive" 2>/dev/null | sed -n '1s|^\./||;1s|/.*||p') || true
    if [ -z "$om_top" ]; then
        om_die "release archive is empty or not a readable tar.gz: $om_archive"
    fi
    mkdir -p "$om_tmp/unpacked-bin"
    tar -xzf "$om_archive" -C "$om_tmp/unpacked-bin" 2>/dev/null ||
        om_die "could not extract the release archive (corrupt download?)"
    if [ ! -d "$om_tmp/unpacked-bin/$om_top" ]; then
        om_die "release archive does not contain the expected top-level directory '$om_top'"
    fi
    om_asset_root="$om_tmp/unpacked-bin/$om_top"
    for om_exe_name in "openmesh-bridge$om_exe" "openmesh-cli$om_exe"; do
        if [ ! -f "$om_asset_root/$om_exe_name" ]; then
            om_die "release archive is missing $om_exe_name.
       The release may predate the binary-asset layout; see docs/INSTALL.md for a
       source build, or report it at https://github.com/$om_repo/issues"
        fi
    done
}

om_unpack_web_asset() {
    om_archive=$1
    mkdir -p "$om_tmp/unpacked-web"
    tar -xzf "$om_archive" -C "$om_tmp/unpacked-web" 2>/dev/null ||
        om_die "could not extract the web bundle (corrupt download?)"
    if [ ! -d "$om_tmp/unpacked-web/dist" ]; then
        om_die "web bundle does not contain the expected 'dist/' directory"
    fi
    om_web_root="$om_tmp/unpacked-web/dist"
}

# --------------------------------------------------------------------------- install actions
om_make_dir() {
    om_dir=$1
    om_mode=$2
    if [ -d "$om_dir" ]; then
        return 0
    fi
    om_run mkdir -p "$om_dir"
    om_run chmod "$om_mode" "$om_dir"
}

om_install_binary() {
    om_name=$1
    om_src=$2
    om_install_file "$om_src" "$om_bin_dir/$om_name" 0755
    om_say "  installed $om_bin_dir/$om_name"
}

om_install_file() {
    om_srcfile=$1
    om_dstfile=$2
    om_mode=$3
    om_run mkdir -p "$(dirname "$om_dstfile")"
    om_run cp "$om_srcfile" "$om_dstfile"
    om_run chmod "$om_mode" "$om_dstfile"
}

om_install_web() {
    if [ "$om_install_web" = 0 ]; then
        return 0
    fi
    om_info "installing web control panel to $om_web_dir"
    if [ "$om_dry_run" = 0 ]; then
        if [ -d "$om_web_dir" ]; then
            rm -rf "$om_web_dir"
        fi
    fi
    om_make_dir "$om_web_dir" 0755
    if [ "$om_dry_run" = 0 ]; then
        cp -R "$om_web_root/." "$om_web_dir/"
        chmod -R u=rwX,go=rX "$om_web_dir"
        om_say "  installed web app in $om_web_dir"
    else
        om_say "[dry-run] would copy the web app to $om_web_dir"
    fi
}

om_install_docs() {
    if [ "$om_install_docs" = 0 ]; then
        return 0
    fi
    om_make_dir "$om_doc_dir" 0755
    for om_doc in LICENSE NOTICE README.md; do
        if [ -f "$om_asset_root/$om_doc" ]; then
            om_install_file "$om_asset_root/$om_doc" "$om_doc_dir/$om_doc" 0644
        fi
    done
    om_say "  installed license/readme in $om_doc_dir"
}

om_prime_dirs() {
    # User installations get writable state/config dirs; system installations let
    # systemd's StateDirectory/ConfigurationDirectory create and own them.
    if [ "$om_system" = 1 ] && [ -z "$om_system_root" ]; then
        om_info "system directories will be managed by systemd (StateDirectory/ConfigurationDirectory)"
        return 0
    fi
    om_make_dir "$om_data_dir" 0700
    om_make_dir "$om_config_dir" 0700
    if [ "$om_dry_run" = 0 ]; then
        case $(uname -s 2>/dev/null || printf unknown) in
            Linux|Darwin)
                chmod 0700 "$om_data_dir" "$om_config_dir" 2>/dev/null || true
                ;;
        esac
    fi
    om_say "  data directory: $om_data_dir"
    om_say "  config directory: $om_config_dir"
}

om_service_unit_body() {
    cat <<EOF
# Managed by OpenMesh install.sh - remove with: install.sh --uninstall
[Unit]
Description=OpenMesh backbone relay (openmesh-bridge)
Documentation=https://github.com/$om_repo/blob/main/docs/INSTALL.md
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=$(om_sd_token "$om_bin_dir/openmesh-bridge") backbone --data-dir $(om_sd_token "$om_data_dir") --listen 127.0.0.1:8787 --serve-web $(om_sd_token "$om_web_dir")
Restart=on-failure
RestartSec=5
TimeoutStopSec=20
WorkingDirectory=$om_data_dir
Environment=RUST_LOG=info
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=$om_data_dir $om_config_dir
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryMax=512M

[Install]
WantedBy=default.target
EOF
}

om_install_service() {
    if [ "$om_service" = 0 ]; then
        return 0
    fi
    if [ "$om_dry_run" = 0 ]; then
        case $(uname -s 2>/dev/null || printf unknown) in
            Linux) : ;;
            *) om_die "--service requires Linux with systemd (this is $om_os)" ;;
        esac
    fi

    om_unit_path="$om_service_dir/$om_unit_name"
    for om_service_path in "$om_bin_dir" "$om_data_dir" "$om_web_dir" "$om_config_dir"; do
        case $om_service_path in
            *" "*)
                om_die "paths containing spaces are not supported by the generated systemd unit
       ($om_service_path). Re-run with --bin-dir / a HOME path without spaces, or install the
       unit manually from packaging/systemd/."
                ;;
        esac
        case $om_service_path in
            /*) : ;;
            *)
                om_die "systemd units need absolute paths; got '$om_service_path'.
       Use an absolute --bin-dir/--data-dir or set HOME to an absolute path."
                ;;
        esac
    done
    om_info "installing systemd user unit: $om_unit_path"
    om_run mkdir -p "$om_service_dir"
    if [ "$om_dry_run" = 0 ]; then
        om_service_tmp="$om_unit_path.tmp.$$"
        om_service_unit_body >"$om_service_tmp"
        chmod 0644 "$om_service_tmp"
        mv "$om_service_tmp" "$om_unit_path"
        om_say "  wrote $om_unit_path"
    else
        om_say "[dry-run] would write the systemd user unit to $om_unit_path"
    fi

    if [ "${OPENMESH_SKIP_SYSTEMCTL:-0}" = "1" ]; then
        om_warn "OPENMESH_SKIP_SYSTEMCTL=1: not enabling the unit"
        return 0
    fi
    if [ "$om_dry_run" = 1 ]; then
        om_say "[dry-run] would run: systemctl --user daemon-reload && systemctl --user enable --now $om_unit_name"
        return 0
    fi
    if om_have systemctl && systemctl --user show-environment >/dev/null 2>&1; then
        systemctl --user daemon-reload || om_warn "systemctl --user daemon-reload failed"
        if systemctl --user enable --now "$om_unit_name"; then
            om_say "  service enabled and started"
        else
            om_warn "could not enable the unit; inspect with: systemctl --user status $om_unit_name"
        fi
        if om_have loginctl; then
            om_say "  hint: run 'loginctl enable-linger $USER' so the backbone stays up after logout"
        fi
    else
        om_warn "no systemd user session detected; the unit was written but not enabled.
       Enable it later with:
         systemctl --user daemon-reload
         systemctl --user enable --now $om_unit_name"
    fi
}

# --------------------------------------------------------------------------- summaries
om_print_summary() {
    om_say ""
    om_say "OpenMesh $om_version installed ($om_platform)."
    om_say ""
    om_say "  openmesh-bridge  $om_bin_dir/openmesh-bridge$om_exe"
    om_say "  openmesh-cli     $om_bin_dir/openmesh-cli$om_exe"
    if [ "$om_install_web" = 1 ]; then
        om_say "  web panel files  $om_web_dir"
    fi
    om_say "  data directory   $om_data_dir"
    om_say "  config directory $om_config_dir"
    if [ "$om_service" = 1 ]; then
        om_say "  systemd unit     $om_service_dir/$om_unit_name"
    fi
    om_say ""
    om_say "Next steps:"
    om_say "  1. Start the backbone (first run generates the token and prints onboarding):"
    om_say "       $om_bin_dir/openmesh-bridge$om_exe backbone --data-dir $om_data_dir \\"
    if [ "$om_install_web" = 1 ]; then
        om_say "         --serve-web $om_web_dir --listen 127.0.0.1:8787"
    else
        om_say "         --listen 127.0.0.1:8787"
    fi
    if [ "$om_service" = 0 ]; then
        om_say "  2. Keep it running across reboots:"
        om_say "       $om_bin_dir/openmesh-bridge$om_exe install-service --backbone"
        om_say "     or re-run this installer with --service"
    else
        om_say "  2. The systemd user service is installed; check it with:"
        om_say "       systemctl --user status $om_unit_name"
    fi
    om_say "  3. Open the panel: http://127.0.0.1:8787/backbone"
    om_say "  4. Allow backbone peers through the firewall (default port 9443/tcp):"
    om_say "       sudo firewall-cmd --add-port=9443/tcp --permanent && sudo firewall-cmd --reload  # Fedora"
    om_say "       sudo ufw allow 9443/tcp                                                       # Debian/Ubuntu"
    om_say ""
    om_say "Docs: https://github.com/$om_repo/blob/main/docs/INSTALL.md"
    om_say "      https://github.com/$om_repo/blob/main/docs/operations/backbone-deployment.md"
    if [ "$om_install_web" = 1 ] && [ "$om_service" = 0 ]; then
        om_say ""
        om_say "Note: the backbone command above serves the panel itself; keep it running in a"
        om_say "      terminal or use --service / install-service for a persistent unit."
    fi
    case ":${PATH:-}:" in
        *":$om_bin_dir:"*) : ;;
        *)
            om_say ""
            om_warn "$om_bin_dir is not on your PATH. Add it, e.g.:"
            om_say "       export PATH=\"$om_bin_dir:\$PATH\""
            ;;
    esac
    om_say ""
}

om_print_dry_run_plan() {
    om_say "OpenMesh installer dry-run plan"
    om_say ""
    om_say "  platform        $om_platform"
    om_say "  release         $om_version (source: ${om_base})"
    om_say "  binary asset    $om_asset"
    if [ "$om_install_web" = 1 ]; then
        om_say "  web asset       $om_web_asset"
    fi
    om_say "  checksum file   SHA256SUMS.txt (downloaded and verified before install)"
    om_say "  binaries        $om_bin_dir/openmesh-bridge$om_exe, openmesh-cli$om_exe"
    if [ "$om_install_web" = 1 ]; then
        om_say "  web bundle      $om_web_dir"
    fi
    om_say "  docs            $om_doc_dir (LICENSE, NOTICE, README.md)"
    om_say "  data/config     $om_data_dir, $om_config_dir"
    if [ "$om_system" = 1 ] && [ -z "$om_system_root" ]; then
        om_say "  privilege       sudo (system install)"
    else
        om_say "  privilege       none (user install)"
    fi
    if [ "$om_service" = 1 ]; then
        om_say "  service         $om_service_dir/$om_unit_name (systemd --user, enable --now)"
    fi
    om_say ""
    om_say "No changes made (--dry-run)."
}

# --------------------------------------------------------------------------- uninstall
om_uninstall() {
    om_detect_platform
    om_resolve_paths
    if [ "$om_system" = 1 ] && [ -z "$om_system_root" ]; then
        om_confirm "Remove OpenMesh binaries and web bundle from system paths using sudo?"
    fi

    om_removed=0
    om_info "uninstalling OpenMesh from $om_bin_dir"
    for om_name in "openmesh-bridge$om_exe" "openmesh-cli$om_exe"; do
        om_path="$om_bin_dir/$om_name"
        if [ -e "$om_path" ]; then
            om_run rm -f "$om_path"
            om_removed=$((om_removed + 1))
            if [ "$om_dry_run" = 1 ]; then
                om_say "  would remove $om_path"
            else
                om_say "  removed $om_path"
            fi
        fi
    done
    if [ -d "$om_web_dir" ]; then
        om_run rm -rf "$om_web_dir"
        om_removed=$((om_removed + 1))
        if [ "$om_dry_run" = 1 ]; then
            om_say "  would remove $om_web_dir"
        else
            om_say "  removed $om_web_dir"
        fi
    fi
    for om_doc in LICENSE NOTICE README.md; do
        if [ -e "$om_doc_dir/$om_doc" ]; then
            om_run rm -f "$om_doc_dir/$om_doc"
            om_removed=$((om_removed + 1))
            if [ "$om_dry_run" = 1 ]; then
                om_say "  would remove $om_doc_dir/$om_doc"
            fi
        fi
    done

    om_unit_path="$om_service_dir/$om_unit_name"
    if [ -e "$om_unit_path" ]; then
        if head -n 1 "$om_unit_path" 2>/dev/null | grep -q "Managed by OpenMesh install.sh"; then
            if [ "${OPENMESH_SKIP_SYSTEMCTL:-0}" != "1" ] && om_have systemctl &&
                systemctl --user show-environment >/dev/null 2>&1; then
                systemctl --user disable --now "$om_unit_name" 2>/dev/null || true
            fi
            om_run rm -f "$om_unit_path"
            om_removed=$((om_removed + 1))
            if [ "$om_dry_run" = 1 ]; then
                om_say "  would remove systemd unit $om_unit_path"
            else
                om_say "  removed systemd unit $om_unit_path"
            fi
            if [ "${OPENMESH_SKIP_SYSTEMCTL:-0}" != "1" ] && om_have systemctl &&
                systemctl --user show-environment >/dev/null 2>&1; then
                systemctl --user daemon-reload 2>/dev/null || true
            fi
        else
            om_warn "leaving $om_unit_path in place: it was not written by this installer"
        fi
    fi

    om_say ""
    if [ "$om_removed" = 0 ]; then
        om_say "Nothing to remove (OpenMesh does not appear to be installed in these paths)."
    elif [ "$om_dry_run" = 1 ]; then
        om_say "Dry-run: nothing was removed."
    else
        om_say "Uninstalled OpenMesh binaries/web bundle."
    fi
    om_say ""
    om_say "Kept (remove manually if you want a clean slate):"
    om_say "  data:   $om_data_dir"
    om_say "  config: $om_config_dir  (bridge API token; delete to invalidate panel access)"
    om_say ""
    om_say "To purge:  rm -rf \"$om_data_dir\" \"$om_config_dir\""
}

# --------------------------------------------------------------------------- main
om_main() {
    om_parse_args "$@"

    if [ "$om_uninstall" = 1 ]; then
        om_uninstall
        return 0
    fi

    om_detect_platform
    om_resolve_paths
    om_check_tools

    if [ "$om_system" = 1 ] && [ -z "$om_system_root" ]; then
        om_confirm "This installs OpenMesh to $om_bin_dir and $om_web_dir using sudo. Continue?"
    fi

    om_tmp=$(mktemp -d "${TMPDIR:-/tmp}/openmesh-install.XXXXXX") ||
        om_die "could not create a temporary directory"

    om_resolve_version

    if [ "$om_dry_run" = 1 ]; then
        om_print_dry_run_plan
        return 0
    fi

    om_check_tools
    om_fetch_release
    om_unpack_binary_asset "$om_tmp/$om_asset"
    if [ "$om_install_web" = 1 ]; then
        om_unpack_web_asset "$om_tmp/$om_web_asset"
    fi

    om_info "installing OpenMesh $om_version"
    om_make_dir "$om_bin_dir" 0755
    om_install_binary "openmesh-bridge$om_exe" "$om_asset_root/openmesh-bridge$om_exe"
    om_install_binary "openmesh-cli$om_exe" "$om_asset_root/openmesh-cli$om_exe"
    om_install_web
    om_install_docs
    om_prime_dirs
    om_install_service
    om_print_summary
}

om_main "$@"
