#!/usr/bin/env bash
set -euo pipefail

# Tauri's AppImage already contains the application and GTK/WebKit files. This
# post-processing step removes host-integration libraries that are unsafe to
# carry across distributions, repairs missing WebKit helpers, and repacks the
# image with a deterministic appimagetool version.

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <AppImage>" >&2
  exit 2
fi

APPIMAGE_INPUT=$1
if [[ ! -f "$APPIMAGE_INPUT" ]]; then
  echo "AppImage not found: $APPIMAGE_INPUT" >&2
  exit 1
fi

APPIMAGE_ABS=$(cd "$(dirname "$APPIMAGE_INPUT")" && pwd)/$(basename "$APPIMAGE_INPUT")
WORKDIR=$(mktemp -d "${TMPDIR:-/tmp}/reader-appimage.XXXXXX")
ROOT="$WORKDIR/squashfs-root"
OUTPUT="$WORKDIR/reader-fixed.AppImage"

cleanup() {
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

chmod +x "$APPIMAGE_ABS"
(
  cd "$WORKDIR"
  APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGE_ABS" --appimage-extract >/dev/null
)

if [[ ! -d "$ROOT" || ! -x "$ROOT/AppRun" ]]; then
  echo "Unable to extract a valid AppImage root" >&2
  exit 1
fi

find_file() {
  local name=$1
  local base
  for base in \
    "$ROOT/usr/lib" \
    "$ROOT/usr/lib/x86_64-linux-gnu" \
    "$ROOT/usr/libexec" \
    "$ROOT/lib/x86_64-linux-gnu"; do
    [[ -d "$base" ]] || continue
    local found
    found=$(find "$base" \( -type f -o -type l \) -name "$name" -perm -u+x -print -quit 2>/dev/null || true)
    if [[ -n "$found" ]]; then
      printf '%s\n' "$found"
      return
    fi
  done
}

find_host_file() {
  local name=$1
  local base
  for base in \
    /usr/lib/x86_64-linux-gnu/webkit2gtk-4.1 \
    /usr/libexec/webkit2gtk-4.1 \
    /usr/lib/webkit2gtk-4.1; do
    [[ -d "$base" ]] || continue
    local found
    found=$(find "$base" \( -type f -o -type l \) -name "$name" -print -quit 2>/dev/null || true)
    if [[ -n "$found" ]]; then
      printf '%s\n' "$found"
      return
    fi
  done
}

find_any_file() {
  local name=$1
  local base
  for base in \
    "$ROOT/usr/lib" \
    "$ROOT/usr/lib/x86_64-linux-gnu" \
    "$ROOT/usr/libexec" \
    "$ROOT/lib/x86_64-linux-gnu"; do
    [[ -d "$base" ]] || continue
    local found
    found=$(find "$base" \( -type f -o -type l \) -name "$name" -print -quit 2>/dev/null || true)
    if [[ -n "$found" ]]; then
      printf '%s\n' "$found"
      return
    fi
  done
}

copy_webkit_helper() {
  local name=$1
  local existing
  existing=$(find_file "$name")
  if [[ -n "$existing" ]]; then
    chmod +x "$existing"
    return
  fi

  local source
  source=$(find_host_file "$name")
  if [[ -z "$source" ]]; then
    echo "Missing WebKit helper: $name" >&2
    echo "Install the WebKitGTK 4.1 development/runtime packages on the build runner." >&2
    exit 1
  fi

  local relative=${source#/usr/}
  local destination="$ROOT/usr/${relative%/*}"
  mkdir -p "$destination"
  cp -f "$source" "$destination/$name"
  chmod +x "$destination/$name"
}

copy_webkit_file() {
  local name=$1
  local existing
  existing=$(find_any_file "$name")
  if [[ -n "$existing" ]]; then
    return
  fi

  local source
  source=$(find_host_file "$name")
  if [[ -z "$source" ]]; then
    echo "Missing WebKit injected bundle: $name" >&2
    echo "Install the WebKitGTK 4.1 development/runtime packages on the build runner." >&2
    exit 1
  fi

  local relative=${source#/usr/}
  local destination="$ROOT/usr/${relative%/*}"
  mkdir -p "$destination"
  cp -f "$source" "$destination/$name"
}

# The helper processes are separate executables. Keep them next to the WebKit
# runtime so WebKitGTK can resolve its relative library paths on each distro.
copy_webkit_helper WebKitNetworkProcess
copy_webkit_helper WebKitWebProcess
copy_webkit_file libwebkit2gtkinjectedbundle.so

# These libraries are tightly coupled to the host kernel, desktop session,
# GLib/GIO installation, and GStreamer registry. Bundling an older copy causes
# errors such as libgvfscommon.so undefined symbols on newer distributions.
for pattern in \
  'libwayland-*.so*' \
  'libglib-2.0.so*' \
  'libgio-2.0.so*' \
  'libgobject-2.0.so*' \
  'libgmodule-2.0.so*' \
  'libgthread-2.0.so*' \
  'libgst*.so*' \
  'libmount.so*' \
  'libblkid.so*' \
  'libselinux.so*' \
  'libpcre2-8.so*' \
  'libzstd.so*' \
  'libelf.so*' \
  'libffi.so*'; do
  find "$ROOT/usr" "$ROOT/lib" -name "$pattern" -delete 2>/dev/null || true
done

APP_BINARY="$ROOT/usr/bin/reader-desktop"
if [[ ! -x "$APP_BINARY" ]]; then
  echo "Tauri application binary not found: $APP_BINARY" >&2
  exit 1
fi

# AppRun.wrapped may reintroduce AppImage-local GStreamer paths after hooks run.
# A shim around the real binary is invoked after that wrapper and can remove
# those paths before GTK/WebKit starts.
mv "$APP_BINARY" "$APP_BINARY.bin"
cat > "$APP_BINARY" <<'SHIM'
#!/usr/bin/env bash
set -euo pipefail

APPDIR=${APPDIR:-$(cd "$(dirname "$0")/.." && pwd)}

case "${GIO_EXTRA_MODULES:-}" in
  "$APPDIR"/*) unset GIO_EXTRA_MODULES ;;
esac
case "${GIO_MODULE_DIR:-}" in
  "$APPDIR"/*) unset GIO_MODULE_DIR ;;
esac
case "${GST_PLUGIN_SYSTEM_PATH_1_0:-}" in
  "$APPDIR"/*) unset GST_PLUGIN_SYSTEM_PATH_1_0 ;;
esac
case "${GST_PLUGIN_PATH_1_0:-}" in
  "$APPDIR"/*) unset GST_PLUGIN_PATH_1_0 ;;
esac

# Use the host GIO implementation and a conservative WebKitGTK renderer.
export GIO_USE_VFS="${GIO_USE_VFS:-local}"
export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"
export WEBKIT_DISABLE_COMPOSITING_MODE="${WEBKIT_DISABLE_COMPOSITING_MODE:-1}"
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"

exec "$APPDIR/usr/bin/reader-desktop.bin" "$@"
SHIM
chmod +x "$APP_BINARY"

APPIMAGETOOL=${APPIMAGETOOL:-}
if [[ -z "$APPIMAGETOOL" ]]; then
  APPIMAGETOOL="$WORKDIR/appimagetool-x86_64.AppImage"
  curl --fail --location --retry 3 --silent --show-error \
    "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage" \
    --output "$APPIMAGETOOL"
  expected_sha256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
  actual_sha256=$(sha256sum "$APPIMAGETOOL" | awk '{print $1}')
  [[ "$actual_sha256" == "$expected_sha256" ]] || {
    echo "appimagetool checksum mismatch" >&2
    exit 1
  }
fi
chmod +x "$APPIMAGETOOL"

(
  cd "$WORKDIR"
  ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" "$ROOT" "$OUTPUT"
)
[[ -s "$OUTPUT" ]] || {
  echo "appimagetool produced an empty image" >&2
  exit 1
}

mv -f "$OUTPUT" "$APPIMAGE_ABS"
chmod +x "$APPIMAGE_ABS"
echo "Fixed AppImage: $APPIMAGE_ABS"
