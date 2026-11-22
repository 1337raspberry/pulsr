# pulsr

<img width="970" height="639" alt="pulsr-loop" src="https://github.com/user-attachments/assets/d93610ef-6ae7-4d9c-a191-7de5c0c6db95" />


it makes squiggly pulsar lines out of whatever your computer is playing. in your terminal.

its very responsive and fun to look at if you enjoy jingly keys and bouncing visualisers as i do. it has many standard community colour schemes built in and it supports custom ones too. it supports standard braille or ASCII characters to work in every terminal i've tested so far, and it also supports octant rendering on modern terminals for a smooth line

written in rust. macos (apple silicon, 14.2+) and linux (pulseaudio or pipewire).

inspired by the visualiser in [plexamp](https://www.plex.tv/en-gb/plexamp/) and the higher fidelity version of this i put together in my other project [ramus](https://github.com/1337raspberry/ramus)

## get it

```sh
brew install 1337raspberry/tap/pulsr
```

or grab a build from releases and put it in your path somewhere (on macos stick with brew, a build downloaded in a browser gets blocked by gatekeeper). or `cargo build --release` if you are like that.

## use it

```sh
pulsr
```

play some music. `q` to quit. `pulsr -help` for the rest

on macos your terminal will ask for audio capture permission the first time. say yes or enjoy a flat line that does nothing. if you miss this then go to system settings > privacy etc and make sure your terminal has audio recording privs

some other hotkeys to play around with:

f - toggle "fullscreen" eg hide your command prompt

m - toggle mode (octant/braille/ascii flat/ascii static) (static isn't a good name for that i am realising, i mean static as in, TV static. o h well)

t - cycle theme

g - cycle gradient colours

r - toggle rising mode (no static bottom line)

s - toggle settle mode (no music playing = motion stops)

-/+ - change fps

,/. - change height

[/] - change row spacing

;/' - change the fade amount

9/0 - fade curve/intensity

also try changing your terminal font size, shrink it down enough and its just a dang line isn't that cool? thats just text doing that.

<img width="1144" height="660" alt="Screenshot 2026-11-19 at 11 57 19" src="https://github.com/user-attachments/assets/97b1799b-cbd6-4342-839d-6e3cd3de6728" />

## configure it

settings live in `~/.config/pulsr/` (or `$XDG_CONFIG_HOME/pulsr/`):

- `config` - your settings, one `key = value` per line, e.g. `theme = nord`.
  keys: theme, gradient, mode, full, rising, settle, fps, lines, pitch,
  ascii-pitch, height, fade, curve.
- `state` - written by pulsr on quitting: whatever the keys last chose.
- `themes/` - your own themes, one file each.

whichever of `config` and `state` was written last wins so you can control and define stuff from external theme files, or interactive choices stay as they most recently were, whatever youre into

command line option always beats both tho

## themes

`themes/NAME.theme` adds a theme called NAME:

```
background = #0b1410
text = #c8d6c0
gradient atomic = #1f6f4a #39d98a #b8fff0
```

all lines are optional. Leave out `background` to keep the terminal's own one

a gradient takes one to three colours, flat rows first, peaks last, and a theme can have several. Pick it with `-theme NAME` when invoking, `theme = NAME` in `config`, or just cycle to it with the T key.

theme name is a single word and cant be 'terminal', if you call it the same as a built in theme, it will override that one.

theme files with a mistake are skipped, if your theme isnt working, quit and check the messages, it'll tell you what line is broken

## live colours

pulsr follows colour changes as it runs, within a second:

- a theme file that is rewritten is read again if you wanna live tweak your theme
- the `terminal` theme follows the terminal's own colours, the text gradient (default) is just your text colour and the `palette` gradient (G key) is built from the terminal's palette.

so with pywal or wallust, the `terminal` theme just works and stays up to date, for more control, have the tool write a theme file. a wallust template:

```
background = {{background}}
text = {{foreground}}
gradient wal = {{color4}} {{color5}} {{color6}}
```

and then in `wallust.toml`:

```
[templates]
pulsr = { template = "pulsr.theme", target = "~/.config/pulsr/themes/wal.theme" }
```

for pywal, the same template with single braces (`{background}`,
`{color4}`), linked from `~/.cache/wal/` into `themes/`.


## license

mit
