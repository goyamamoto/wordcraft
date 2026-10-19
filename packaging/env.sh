# shellcheck shell=bash
# Shared setup for the packaging scripts. Source it: `. "$(dirname "$0")/../env.sh"`.
#
# Exports:
#   ROOT                    workspace root
#   VERSION                 [workspace.package] version from Cargo.toml (override: WORDCRAFT_VERSION)
#   DIST                    output directory for release artifacts (default: $ROOT/dist/release)
#   WORDCRAFT_BUILD_SHA    git commit baked into the binaries (see crates/engine/src/build_info.rs)
#   WORDCRAFT_BUILD_DATE   UTC build date, YYYY-MM-DD
#   CARGO_TARGET_DIR        cargo's target dir (default: $ROOT/target)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in exactly one place: `[workspace.package] version` in the root Cargo.toml.
# (`cargo xtask version` prints the same thing; awk avoids compiling xtask here.)
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") ; next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
}

VERSION="${WORDCRAFT_VERSION:-$(workspace_version)}"
if [ -z "$VERSION" ]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$DIST"
export DIST

if [ -z "${WORDCRAFT_BUILD_SHA:-}" ]; then
  WORDCRAFT_BUILD_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || true)"
fi
export WORDCRAFT_BUILD_SHA
export WORDCRAFT_BUILD_DATE="${WORDCRAFT_BUILD_DATE:-$(date -u +%Y-%m-%d)}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

# Emit a GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
}

# Copy licence and readme files that exist into a package directory.
copy_docs() {
  local dest="$1" f
  for f in README.md LICENSE LICENSE-MIT LICENSE-APACHE COPYRIGHT; do
    if [ -f "$ROOT/$f" ]; then cp "$ROOT/$f" "$dest/"; fi
  done
  copy_font_licences "$dest"
}

# Builds made with craft-fonts (CRAFT_FONTS_DIR, set by the release workflow) embed its fonts, so
# the package carries their licences as OFL-<family-dir>.txt. Nothing without CRAFT_FONTS_DIR.
copy_font_licences() {
  local dest="$1" f
  [ -n "${CRAFT_FONTS_DIR:-}" ] || return 0
  for f in "$CRAFT_FONTS_DIR"/fonts/*/OFL.txt; do
    if [ -f "$f" ]; then cp "$f" "$dest/OFL-$(basename "$(dirname "$f")").txt"; fi
  done
}

# The Japanese dictionary wordcraft-jaseg reads at run time for Japanese words (a double-click):
# lindera's prebuilt IPADIC (mecab-ipadic 2.7.0; NAIST/ICOT licence, which allows redistribution
# with its notice — the folder's NOTICE.txt ships with it), pinned by version and SHA-256.
JA_DICT_URL="https://github.com/lindera/lindera/releases/download/v6.2.0/lindera-ipadic-6.2.0.zip"
JA_DICT_SHA256="5ed4bba6b429030b0387df67d40d5751d35dedef0306f4bac6d957cad5f04b72"

# Copy the dictionary to DEST/lindera-ipadic: from JA_DICT_DIR (an unpacked lindera-ipadic folder)
# when set, else from the pinned release zip, downloaded once into $CARGO_TARGET_DIR/ja-dict and
# checked against JA_DICT_SHA256. WORDCRAFT_NO_JA_DICT=1 packages without it (Japanese
# double-clicks then select the whole run of kana and kanji).
copy_ja_dict() {
  local dest="$1" cache zip src
  if [ -n "${WORDCRAFT_NO_JA_DICT:-}" ]; then
    warn "packaging without the Japanese dictionary (WORDCRAFT_NO_JA_DICT)"
    return 0
  fi
  if [ -n "${JA_DICT_DIR:-}" ]; then
    src="$JA_DICT_DIR"
  else
    cache="$CARGO_TARGET_DIR/ja-dict"
    zip="$cache/$(basename "$JA_DICT_URL")"
    mkdir -p "$cache"
    if [ ! -f "$zip" ] || [ "$(sha256 "$zip")" != "$JA_DICT_SHA256" ]; then
      if ! curl -fsSL --retry 3 -o "$zip.part" "$JA_DICT_URL"; then
        echo "error: couldn't download $JA_DICT_URL (or set JA_DICT_DIR to an unpacked lindera-ipadic)" >&2
        exit 1
      fi
      mv -f "$zip.part" "$zip"
    fi
    if [ "$(sha256 "$zip")" != "$JA_DICT_SHA256" ]; then
      echo "error: $zip doesn't match its pinned SHA-256" >&2
      exit 1
    fi
    rm -rf "$cache/unpacked"
    mkdir -p "$cache/unpacked"
    unzip -q "$zip" -d "$cache/unpacked"
    src="$cache/unpacked/lindera-ipadic"
  fi
  if [ ! -f "$src/dict.trie" ] || [ ! -f "$src/NOTICE.txt" ]; then
    echo "error: $src isn't a lindera dictionary with its NOTICE.txt" >&2
    exit 1
  fi
  mkdir -p "$dest"
  rm -rf "$dest/lindera-ipadic"
  cp -R "$src" "$dest/lindera-ipadic"
}

# Portable SHA-256 of a file (prints just the hash).
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
