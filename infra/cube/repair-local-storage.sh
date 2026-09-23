#!/usr/bin/env bash
set -euo pipefail

# Recover the existing image only. Never format storage or replace existing data.
[[ $EUID == 0 ]] || { echo 'Run as root'; exit 1; }
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
old_image=/var/lib/mindloop/cubesandbox/cubelet-xfs.img
image=/var/lib/cubesandbox/cubelet-xfs.img
if mountpoint -q /data/cubelet; then
  echo 'Cubelet storage is already mounted; inspect before proceeding.'
  exit 1
fi
[[ -f "$old_image" && ! -e "$image" ]]
[[ $(curl --fail --silent http://127.0.0.1:3001/health) == *'"sandboxes":0'* ]]
[[ -z $(losetup -j "$old_image") ]]
systemctl stop cube-sandbox-cubelet.service cube-sandbox-network-agent.service
install -d -m 0755 /var/lib/cubesandbox
mv -- "$old_image" "$image"
install -m 0644 "$source_dir/data-cubelet.mount" /etc/systemd/system/data-cubelet.mount
install -d /etc/systemd/system/cube-sandbox-network-agent.service.d
install -m 0644 "$source_dir/storage-dependency.conf" /etc/systemd/system/cube-sandbox-network-agent.service.d/storage.conf
systemctl daemon-reload
systemctl enable --now data-cubelet.mount
findmnt -T /data/cubelet -o TARGET,SOURCE,FSTYPE,OPTIONS
xfs_info /data/cubelet
