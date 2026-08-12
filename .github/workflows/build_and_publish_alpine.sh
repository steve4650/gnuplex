#!/bin/sh
set -e

umask 077
echo "$1" >/tmp/gnuplex-deploy
GNUPLEX_GO_VERSION="$2"

apk add uv git curl bash openssh gcc musl-dev libstdc++ unzip

cd /usr/local/bin
wget "https://go.dev/dl/go$GNUPLEX_GO_VERSION.linux-amd64.tar.gz"
tar xvzf go*.gz
mv go godir
ln -s godir/bin/go .
ln -s godir/bin/gofmt .

wget https://github.com/oven-sh/bun/releases/latest/download/bun-linux-x64-musl.zip
unzip bun-linux-x64-musl.zip
mv bun-linux-x64-musl/bun .

export GIT_SSH_COMMAND="ssh -i /tmp/gnuplex-deploy -o StrictHostKeyChecking=no"

cd /
git clone git@github.com:janie314/gnuplex.git
cd gnuplex
git config user.name "release workflow"
git config user.email "x@example.com"
git checkout release-linux-musl-x86_64
git pull origin release-linux-musl-x86_64

git merge main -X theirs --allow-unrelated-histories

export CGO_ENABLED=1
uv run make.py build
version_output=$(./backend/bin/gnuplex -version)
git add .
git commit -m "$version_output"

git push origin release-linux-musl-x86_64