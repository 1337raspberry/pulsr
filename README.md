# pulsr

<img width="581" height="936" alt="Screenshot 2026-11-19 at 11 45 36" src="https://github.com/user-attachments/assets/5590f8db-c196-4b1a-8fbc-d71b351e6518" />


it makes squiggly pulsar lines out of whatever your computer is playing. in your terminal.

its very responsive and fun to look at if you enjoy jingly keys and bouncing visualisers as i do. it has many standard community colour schemes built in and it supports custom ones too. it supports standard braille characters to work in every terminal i've tested so far, and it also supports octant rendering on modern terminals for a smooth line

written in rust. macos (apple silicon, 14.2+) and linux (pulseaudio or pipewire).

inspired by the visualiser in [plexamp](https://www.plex.tv/en-gb/plexamp/) and the higher fidelity version of this i put together in my other project [ramus](https://github.com/1337raspberry/ramus)

## get it

```sh
brew install 1337raspberry/tap/pulsr
```

or grab a build from releases and put it in your path somewhere. or `cargo build --release` if you are like that.

## use it

```sh
pulsr
```

play some music. `q` to quit. `pulsr -help` for the rest

on macos your terminal will ask for audio capture permission the first time. say yes or enjoy a flat line that does nothing

some other hotkeys to play around with:

f - toggle fullscreen

m - toggle mode (octant or braille)

t - cycle theme

g - cycle gradient colours

-/+ - change fps

,/. - change height

[/] - change row spacing

also try changing your terminal font size, shrink it down enough and its just a dang line isn't that cool? thats just text doing that.

<img width="1144" height="660" alt="Screenshot 2026-11-19 at 11 57 19" src="https://github.com/user-attachments/assets/97b1799b-cbd6-4342-839d-6e3cd3de6728" />

## license

mit
