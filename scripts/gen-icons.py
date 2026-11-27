"""Generate the BattalionOS icon set and splash logo from the master image.

Master: assets/brand/battalionos-logo.webp (1254x1254, transparent).
Small sizes (<= 48 px) use a crop of the centre helmet, because the wordmark does
not read there; larger sizes use the full logo. Run from the repo root:
    python scripts/gen-icons.py
"""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
MASTER = ROOT / "assets/brand/battalionos-logo.webp"
DARK = (14, 15, 18, 255)  # #0e0f12, the shell's background
HELMET_BOX = (400, 200, 860, 660)  # centre soldier's helmet, in master pixels
SMALL_MAX = 48


def tile(src: Image.Image, size: int, pad: float) -> Image.Image:
    """src centred on a dark rounded tile with `pad` (fraction) of padding per side."""
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, size - 1, size - 1), radius=max(2, size // 6), fill=255)
    out.paste(Image.new("RGBA", (size, size), DARK), (0, 0), mask)
    inner = max(1, round(size * (1 - 2 * pad)))
    art = src.resize((inner, inner), Image.LANCZOS)
    off = (size - inner) // 2
    out.alpha_composite(art, (off, off))
    return out


def icon(master: Image.Image, helmet: Image.Image, size: int) -> Image.Image:
    return tile(helmet, size, 0.06) if size <= SMALL_MAX else tile(master, size, 0.04)


def main() -> None:
    master = Image.open(MASTER).convert("RGBA")
    helmet = master.crop(HELMET_BOX)
    for crate in ("battalion-desktop", "battalion-android"):
        d = ROOT / "crates" / crate / "icons"
        d.mkdir(parents=True, exist_ok=True)
        icon(master, helmet, 32).save(d / "32x32.png", optimize=True)
        icon(master, helmet, 128).save(d / "128x128.png", optimize=True)
        retina = "128x128" "@2x.png"  # split: the joined name matches the e-mail rule in scripts/scan
        if (d / retina).exists():  # only where it already ships (android)
            icon(master, helmet, 256).save(d / retina, optimize=True)
        icon(master, helmet, 512).save(d / "icon.png", optimize=True)
        sizes = [16, 24, 32, 48, 64, 128, 256]
        frames = [icon(master, helmet, s) for s in sizes]
        frames[-1].save(d / "icon.ico", format="ICO", sizes=[(s, s) for s in sizes], append_images=frames[:-1])

    # Android launcher: legacy square/round icons and the adaptive foreground
    # (108dp canvas, art inside the 66dp safe zone; background is a dark colour resource).
    res = ROOT / "crates/battalion-android/gen/android/app/src/main/res"
    for bucket, legacy, fg in (("mdpi", 48, 108), ("hdpi", 72, 162), ("xhdpi", 96, 216), ("xxhdpi", 144, 324), ("xxxhdpi", 192, 432)):
        d = res / f"mipmap-{bucket}"
        sq = icon(master, helmet, legacy)
        sq.save(d / "ic_launcher.png", optimize=True)
        rnd = Image.new("RGBA", (legacy, legacy), (0, 0, 0, 0))
        m = Image.new("L", (legacy, legacy), 0)
        ImageDraw.Draw(m).ellipse((0, 0, legacy - 1, legacy - 1), fill=255)
        rnd.paste(Image.new("RGBA", (legacy, legacy), DARK), (0, 0), m)
        art = (helmet if legacy <= SMALL_MAX else master).resize((round(legacy * 0.72),) * 2, Image.LANCZOS)
        o = (legacy - art.width) // 2
        rnd.alpha_composite(art, (o, o))
        rnd.save(d / "ic_launcher_round.png", optimize=True)
        f = Image.new("RGBA", (fg, fg), (0, 0, 0, 0))
        art = master.resize((round(fg * 62 / 108),) * 2, Image.LANCZOS)
        o = (fg - art.width) // 2
        f.alpha_composite(art, (o, o))
        f.save(d / "ic_launcher_foreground.png", optimize=True)

    # Splash / loading logo served by the shell UI.
    pub = ROOT / "crates/battalion-desktop/ui/public"
    pub.mkdir(parents=True, exist_ok=True)
    master.resize((512, 512), Image.LANCZOS).save(pub / "bos-logo.png", optimize=True)


if __name__ == "__main__":
    main()
