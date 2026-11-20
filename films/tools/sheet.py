"""sheet.py <out.png> <light frames…> -- <dark frames…>: a contact sheet, one scene per row, light left, dark right."""
import sys
from PIL import Image, ImageDraw
out, rest = sys.argv[1], sys.argv[2:]
k = rest.index("--")
light, dark = rest[:k], rest[k + 1:]
W, H, G = 640, 360, 8
sheet = Image.new("RGB", (2 * W + 3 * G, len(light) * (H + G) + G), "#888888")
for r, pair in enumerate(zip(light, dark)):
    for c, f in enumerate(pair):
        im = Image.open(f).convert("RGB").resize((W, H), Image.LANCZOS)
        sheet.paste(im, (G + c * (W + G), G + r * (H + G)))
    ImageDraw.Draw(sheet).text((G + 6, G + r * (H + G) + 4), f"scene {r + 1}", fill="#d02020")
sheet.save(out)
