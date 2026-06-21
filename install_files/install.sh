#!/bin/sh
set -e
PATH="$PATH:$HOME/.local/bin"
echo "installing GNUPlex..."
# dependencies
echo "checking if GNUPlex exists; quitting if so. run \`gnuplex -upgrade\` to update GNUPlex instead"
if command -v gnuplex
then
  exit 1
fi
echo "checking if git exists; quitting if not"
command -v git 
echo "checking if curl exists; quitting if not"
command -v curl
# continue
echo "All needed commands exist, continuing..."
printf "Install dir (default %s/.local/bin): " "$HOME"
read -r install_dir
if [ "$install_dir" = "" ]
then
  install_dir="$HOME/.local/bin"
fi
mkdir -p "$install_dir"
# find if glibc or musl
libc=$(ldd /bin/ls | grep 'musl' | head -1 | cut -d ' ' -f1)
if [ -z "$libc" ]
then
  libc="glibc"
else
  libc="musl"
fi
# find if x86_64
if [ "$(uname --machine)" != "x86_64" ]
then
  echo "Unsupported architecture: $(uname --machine)"
  exit 1
fi
# clone from git
git clone -b "release-linux-$libc-x86_64" https://github.com/steve4650/gnuplex.git "$install_dir/gnuplex-code"
cd "$install_dir"
ln -s gnuplex-code/backend/bin/gnuplex .
# systemd service
if command -v journalctl
then
  echo "Installing systemd user service..."
  mkdir -p "$HOME/.config/systemd/user"
  install_dir_replace=$(echo "$install_dir" | sed -e 's~/~\\/~g')
  sed -e "s/__DIR__/$install_dir_replace/" "$install_dir/gnuplex-code/install_files/gnuplex.service" >"$HOME/.config/systemd/user/gnuplex.service"
  printf "\n"
  echo "Done."
  printf "\n\n" 
  echo "To run GNUPlex persistently, run \`systemctl --user enable --now gnuplex\` and navigate to http://localhost:40000/"
  printf "\n\n"
  echo "Start GNUPlex ad-hoc with \`$install_dir/gnuplex"\`
else
  echo "To start GNUPlex, run \`$install_dir/gnuplex\` and navigate to http://localhost:40000/."
fi
printf "\n\n" 
