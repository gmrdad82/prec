#!/bin/sh
set -eu
rm -rf "$2"
mkdir -p "$2"
cp -R "$1/examples/demo/render" "$2/render"
cd "$2"
git init -q
git add render
git -c user.name=demo -c user.email=demo@example.com -c commit.gpgsign=false commit -q -m demo
