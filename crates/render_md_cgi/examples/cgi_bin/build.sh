#!/usr/bin/env bash
set -e
cargo build --release
mkdir -p /var/www/cgi-bin
cp target/release/render_md /var/www/cgi-bin/render_md
chmod +x /var/www/cgi-bin/render_md
mkdir -p /var/www/public/assets
cp static/* /var/www/public/assets
tailwindcss -i src/styles/tailwind.css -o /var/www/public/assets/style.css --minify --silent