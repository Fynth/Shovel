#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <version> <repo-dir> <repo-url>" >&2
  exit 1
fi

version="$1"
repo_dir="$2"
repo_url="$3"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "${script_dir}/.." && pwd)"
app_id="dev.shovel.app"
manifest_rel="packaging/flatpak/${app_id}.yml"
build_root="$(mktemp -d)"
source_root="${build_root}/src"
state_root="${build_root}/build"

mkdir -p "${source_root}"
rm -rf "${repo_dir}"
mkdir -p "${repo_dir}"

cleanup() {
  rm -rf "${build_root}"
}
trap cleanup EXIT

git -C "${project_root}" archive --format=tar HEAD | tar -xf - -C "${source_root}"
mkdir -p "${source_root}/.cargo"

(
  cd "${source_root}"
  cargo vendor --locked --versioned-dirs vendor > .cargo/config.toml
)

flatpak-builder \
  --force-clean \
  --default-branch=stable \
  --repo="${repo_dir}" \
  "${state_root}" \
  "${source_root}/${manifest_rel}"

flatpak build-update-repo \
  --generate-static-deltas \
  --title="Shovel Flatpak Repository" \
  "${repo_dir}"

cat > "${repo_dir}/shovel.flatpakrepo" <<EOF
[Flatpak Repo]
Title=Shovel Flatpak Repository
Comment=Flatpak repository for Shovel ${version}
Url=${repo_url}
Homepage=https://github.com/Fynth/Shovel
Icon=https://raw.githubusercontent.com/Fynth/Shovel/refs/heads/main/app/assets/icon.png
DefaultBranch=stable
GPGVerify=false
EOF

cat > "${repo_dir}/index.html" <<EOF
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Shovel Flatpak Repository</title>
</head>
<body>
  <h1>Shovel Flatpak Repository</h1>
  <p>Add the repository:</p>
  <pre><code>flatpak remote-add --user --if-not-exists shovel-flatpak ${repo_url}/shovel.flatpakrepo
flatpak install --user shovel-flatpak ${app_id}</code></pre>
</body>
</html>
EOF
