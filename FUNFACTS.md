# Fun Facts

Random things learned along the way that aren't really part of the app itself —
mostly quirks of the remote hosts/environments this app talks to, kept here so they
don't get rediscovered from scratch later.

## Terminal colors differ per host — that's the remote shell, not us

Two SSH targets can show wildly different `ls` output in the Terminal tab: one plain
white, the other fully colorized (blue dirs, cyan symlinks, green executables). This
has nothing to do with World Keeper's terminal — every session gets the identical PTY
request (`xterm-256color`) and the identical xterm.js config. It's purely whether the
remote shell itself emits ANSI color codes.

Confirmed directly on the reference LXC (`root@10.0.0.68`, Debian/Ubuntu-based): its
`root`'s `~/.bashrc` ships with color `ls` present but commented out — a stock
Debian/Ubuntu default. Regular user accounts get colorized `ls` out of the box; `root`
doesn't.

```
/root/.bashrc:9:# You may uncomment the following lines if you want `ls' to be colorized:
/root/.bashrc:10:# export LS_OPTIONS='--color=auto'
/root/.bashrc:11:# eval "$(dircolors)"
```

The Unraid box's root shell apparently has the equivalent enabled by default (or bakes
color-ls into its base image some other way), which is why that terminal session looks
noticeably nicer with zero effort.

**To enable it on a host like the reference LXC**, uncomment those three lines (and the
`alias ls=` line right below them that references `$LS_OPTIONS`) in `~/.bashrc`, e.g.:

```bash
sed -i \
  -e "s/^# export LS_OPTIONS/export LS_OPTIONS/" \
  -e 's/^# eval "\$(dircolors)"/eval "$(dircolors)"/' \
  -e "s/^alias ls='ls \$LS_OPTIONS'/alias ls='ls \$LS_OPTIONS --color=auto'/" \
  ~/.bashrc
```

Then reconnect the Terminal tab (or run `source ~/.bashrc`) to see it take effect.
