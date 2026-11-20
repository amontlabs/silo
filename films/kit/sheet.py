"""sheet.py <out.png> <frame.png>…: frames side by side in a 2-column sheet, stage area only, for a quick look."""
import sys
from PIL import Image
out, files = sys.argv[1], sys.argv[2:]
ims = [Image.open(f).convert("RGB") for f in files]
ims = [im.crop((0, 0, im.width, int(im.height * 0.94))) for im in ims]
w, h = ims[0].size
cols = 2; rows = (len(ims) + 1) // 2
sheet = Image.new("RGB", (w * cols, h * rows), "white")
for i, im in enumerate(ims): sheet.paste(im, ((i % cols) * w, (i // cols) * h))
sheet.save(out)
