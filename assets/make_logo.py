from PIL import Image, ImageDraw, ImageFilter, ImageChops
import sys
S = 1024
OUT = sys.argv[1]

def vgrad(top, bottom):
    g = Image.new("RGBA", (S, S))
    d = ImageDraw.Draw(g)
    for y in range(S):
        t = y / S
        d.line([(0, y), (S, y)], fill=tuple(int(top[i] * (1 - t) + bottom[i] * t) for i in range(len(top))))
    return g

# Fundo da A: azul-noite, com um brilho azul suave atrás da lua (para o vidro ter o que refratar).
icon = vgrad((14, 48, 78, 255), (4, 12, 24, 255))
aura = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(aura).ellipse((140, 160, 760, 840), fill=(43, 180, 255, 140))
ImageDraw.Draw(aura).ellipse((560, 560, 900, 900), fill=(120, 90, 255, 60))
icon.alpha_composite(aura.filter(ImageFilter.GaussianBlur(120)))

# Máscara da lua crescente.
moon = Image.new("L", (S, S), 0)
md = ImageDraw.Draw(moon)
md.ellipse((215, 167, 899, 851), fill=255)
md.ellipse((415, 67, 1055, 707), fill=0)
moon = moon.filter(ImageFilter.GaussianBlur(1.5))

# Liquid Glass: o fundo desfocado e clareado dentro da lua (cinza claro translúcido)…
blurred = icon.filter(ImageFilter.GaussianBlur(28))
frost = Image.new("RGBA", (S, S), (225, 230, 238, 255))
glass = Image.blend(blurred, frost, 0.5)
# …brilho de cima para baixo (mais claro no alto)…
sheen = vgrad((255, 255, 255, 120), (255, 255, 255, 0))
glass.alpha_composite(sheen)
# …sombra interna perto da borda (dá volume)…
inner = ImageChops.subtract(moon, moon.filter(ImageFilter.MinFilter(41)).filter(ImageFilter.GaussianBlur(18)))
shade = Image.new("RGBA", (S, S), (90, 105, 130, 0))
shade.putalpha(inner.point(lambda v: int(v * 0.55)))
glass.alpha_composite(shade)

# Sombra suave da lua no fundo.
shadow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
shadow.putalpha(moon.point(lambda v: int(v * 0.55)))
shadow = ImageChops.offset(shadow, 0, 22).filter(ImageFilter.GaussianBlur(30))
icon.alpha_composite(shadow)
icon.paste(glass, (0, 0), moon)

# Borda de luz (rim) do vidro: fina, mais forte no alto à esquerda.
edge = ImageChops.subtract(moon, moon.filter(ImageFilter.MinFilter(11)))
rim = vgrad((255, 255, 255, 255), (255, 255, 255, 90))
rl = Image.new("RGBA", (S, S), (0, 0, 0, 0))
rl.paste(rim, (0, 0), edge)
icon.alpha_composite(rl)

# Reflexo especular no alto da lua.
spec = Image.new("L", (S, S), 0)
ImageDraw.Draw(spec).ellipse((275, 277, 475, 417), fill=190)
ImageDraw.Draw(spec).ellipse((605, 717, 745, 777), fill=110)
spec = ImageChops.multiply(spec.filter(ImageFilter.GaussianBlur(30)), moon)
sl = Image.new("RGBA", (S, S), (255, 255, 255, 0))
sl.putalpha(spec)
icon.alpha_composite(sl)

# Cantos arredondados do ícone.
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle((0, 0, S - 1, S - 1), 230, fill=255)
icon.putalpha(ImageChops.multiply(icon.getchannel("A"), mask))
icon.save(f"{OUT}/logo_ab.png")

sheet = Image.new("RGBA", (560, 420), (240, 240, 240, 255))
sheet.alpha_composite(icon.resize((300, 300), Image.LANCZOS), (20, 20))
dark = Image.new("RGBA", (200, 300), (30, 30, 30, 255))
sheet.alpha_composite(dark, (340, 20))
for j, sz in enumerate((64, 48, 32, 16)):
    sheet.alpha_composite(icon.resize((sz, sz), Image.LANCZOS), (20 + j * 80, 340))
    sheet.alpha_composite(icon.resize((sz, sz), Image.LANCZOS), (380, 40 + j * 70))
sheet.save(f"{OUT}/logo_ab_sheet.png")
print("ok")
