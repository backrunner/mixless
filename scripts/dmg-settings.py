# dmgbuild settings for the Mixless installer image.
# Passed via -D: app=<path to Mixless.app>, background=<path to background.png>.
import os

defines = globals().get("defines", {})
app = defines.get("app") or os.environ["MIXLESS_DMG_APP"]
background = defines.get("background") or os.environ.get("MIXLESS_DMG_BACKGROUND")

format = "UDZO"
files = [(app, "Mixless.app")]
symlinks = {"Applications": "/Applications"}

window_rect = ((200, 120), (660, 400))
default_view = "icon-view"
icon_size = 128
text_size = 12
hide_extensions = ["Mixless.app"]
icon_locations = {
    "Mixless.app": (180, 168),
    "Applications": (480, 168),
}
# `background` is already bound above; dmgbuild treats None as no background.
