#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <version> <output-dir>" >&2
  exit 1
fi

version="$1"
output_dir="$2"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "${script_dir}/.." && pwd)"
pkgrel="$("${script_dir}/resolve-pkgrel.sh" "${version}" deb)"
package_version="${version}-${pkgrel}"
package_root="${output_dir}/shovel_${package_version}_amd64"
deb_path="${output_dir}/shovel_${package_version}_amd64.deb"

rm -rf "${package_root}"
mkdir -p "${package_root}/DEBIAN"
mkdir -p "${package_root}/usr/bin"
mkdir -p "${package_root}/usr/lib/shovel/assets"
mkdir -p "${package_root}/usr/share/applications"
mkdir -p "${package_root}/usr/share/icons/hicolor/scalable/apps"
mkdir -p "${package_root}/usr/share/doc/shovel"

cat > "${package_root}/DEBIAN/control" <<EOF
Package: shovel
Version: ${package_version}
Section: database
Priority: optional
Architecture: amd64
Depends: libgtk-3-0, libwebkit2gtk-4.1-0, libjavascriptcoregtk-4.1-0, libsoup-3.0-0, libxdo3
Maintainer: Shovel Maintainers <opensource@shovel.app>
Homepage: https://github.com/Fynth/Shovel
Description: Fast native desktop database client built with Rust and Dioxus
 Shovel is a native desktop database client for SQLite, PostgreSQL, MySQL, and ClickHouse.
 It includes an explorer, SQL editor, result grid, and ACP-powered assistant workflows.
EOF

install -Dm755 "${project_root}/target/release/app" "${package_root}/usr/bin/shovel"
install -Dm644 "${project_root}/app/assets/app.css" \
  "${package_root}/usr/lib/shovel/assets/app.css"
install -Dm644 "${project_root}/packaging/arch/shovel.desktop" \
  "${package_root}/usr/share/applications/shovel.desktop"
install -Dm644 "${project_root}/app/assets/icon.svg" \
  "${package_root}/usr/share/icons/hicolor/scalable/apps/shovel.svg"
install -Dm644 "${project_root}/README.md" \
  "${package_root}/usr/share/doc/shovel/README.md"

dpkg-deb --build --root-owner-group "${package_root}" "${deb_path}"
echo "${deb_path}"
