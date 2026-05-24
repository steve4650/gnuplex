# GNUPlex

GNUPlex is a lightweight personal Netflix that can be run on a Linux PC.

# Installation and Updating

GNUPlex is only built for Linux at the moment. To install it:

1. Install [mpv](https://mpv.io/).

2. Run the following:

```shell
sh <(curl -sL https://gnuplex.davisgroup.uk/install.sh)
```

This will prompt you for a directory in your PATH, then install GNUPlex there.

3. Now you can run `./gnuplex` from your installation directory and navigate to
   `https://localhost:40000/`. To run GNUPlex persistently, run
   `systemctl --user enable --now gnuplex`.

To update GNUPlex:

```shell
gnuplex -upgrade
```

# More Info

[See the wiki.](https://github.com/steve4650/gnuplex/wiki)
