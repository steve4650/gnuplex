#!/bin/sh
set -e
set -x

umask 077
echo "$1" >/tmp/gnuplex-deploy

apk add uv git curl bash openssh gcc musl-dev libstdc++ unzip xz

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
  sh -s -- -y --profile minimal --default-toolchain stable
export PATH="$HOME/.cargo/bin:$PATH"

wget https://github.com/oven-sh/bun/releases/latest/download/bun-linux-x64-musl.zip
unzip bun-linux-x64-musl.zip
mv bun-linux-x64-musl/bun /usr/local/bin/bun

export GIT_SSH_COMMAND="ssh -i /tmp/gnuplex-deploy -o StrictHostKeyChecking=no"

cd /
git clone git@github.com:steve4650/gnuplex.git
cd gnuplex
git config user.name "release workflow"
git config user.email "x@example.com"
git checkout release-linux-musl-x86_64
git pull origin release-linux-musl-x86_64

git merge main -X theirs --allow-unrelated-histories

uv run make.py build
version_output=$(./backend/bin/gnuplex -version)
git add .
git add -f backend/bin backend/static
git commit -m "$version_output"

git push origin release-linux-musl-x86_64
