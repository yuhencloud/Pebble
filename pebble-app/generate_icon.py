from PIL import Image, ImageDraw
import math
import os

def rounded_rect_mask(size, radius):
    mask = Image.new('L', size, 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle((0, 0, size[0]-1, size[1]-1), radius=radius, fill=255)
    return mask

def draw_capybara_icon(size):
    # macOS big-sur style rounded square background
    bg_color = (230, 210, 185)
    radius = max(1, int(size * 0.2247))

    # Base image
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    mask = rounded_rect_mask((size, size), radius)
    bg = Image.new('RGBA', (size, size), (*bg_color, 255))
    img.paste(bg, (0, 0), mask)

    # Colors matching the pixel-art sprite
    body_color = (166, 124, 82)
    ear_color = (196, 154, 112)
    eye_color = (26, 22, 37)
    leg_color = body_color

    # 12x10 sprite with 2-char wide eyes so they always render as solid blocks
    sprite = [
        "  ee    ee  ",
        " bbbbbbbbbb ",
        "boobbbbbboob",
        "bbbbbbbbbbbb",
        "bbbbbbbbbbbb",
        "bbbbbbbbbbbb",
        " bbbbbbbbbb ",
        "  bbbbbbbb  ",
        " l l ll l l ",
        " l l ll l l ",
    ]

    cols = len(sprite[0])
    rows = len(sprite)

    # Choose pixel_block so the sprite fills ~55% of the icon
    pixel_block = max(2, round(size * 0.55 / cols))
    sprite_w = cols * pixel_block
    sprite_h = rows * pixel_block

    cx = (size - sprite_w) // 2
    cy = (size - sprite_h) // 2 + 1  # nudge down slightly for visual balance

    for row_idx, row in enumerate(sprite):
        for col_idx, ch in enumerate(row):
            if ch == ' ':
                continue
            x1 = cx + col_idx * pixel_block
            y1 = cy + row_idx * pixel_block
            x2 = x1 + pixel_block - 1
            y2 = y1 + pixel_block - 1
            if ch == 'e':
                color = ear_color
            elif ch == 'b':
                color = body_color
            elif ch == 'o':
                color = eye_color
            elif ch == 'l':
                color = leg_color
            else:
                continue
            draw = ImageDraw.Draw(img)
            draw.rectangle([x1, y1, x2, y2], fill=(*color, 255))

    return img

def draw_tray_icon(size):
    """Draw the capybara sprite on a transparent background (no border)."""
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    body_color = (166, 124, 82)
    ear_color = (196, 154, 112)
    eye_color = (26, 22, 37)
    leg_color = body_color

    sprite = [
        "  ee    ee  ",
        " bbbbbbbbbb ",
        "boobbbbbboob",
        "bbbbbbbbbbbb",
        "bbbbbbbbbbbb",
        "bbbbbbbbbbbb",
        " bbbbbbbbbb ",
        "  bbbbbbbb  ",
        " l l ll l l ",
        " l l ll l l ",
    ]

    cols = len(sprite[0])
    rows = len(sprite)
    pixel_block = max(2, round(size * 0.55 / cols))
    sprite_w = cols * pixel_block
    sprite_h = rows * pixel_block
    cx = (size - sprite_w) // 2
    cy = (size - sprite_h) // 2 + 1

    for row_idx, row in enumerate(sprite):
        for col_idx, ch in enumerate(row):
            if ch == ' ':
                continue
            x1 = cx + col_idx * pixel_block
            y1 = cy + row_idx * pixel_block
            x2 = x1 + pixel_block - 1
            y2 = y1 + pixel_block - 1
            if ch == 'e':
                color = ear_color
            elif ch == 'b' or ch == 'l':
                color = leg_color
            elif ch == 'o':
                color = eye_color
            else:
                continue
            draw = ImageDraw.Draw(img)
            draw.rectangle([x1, y1, x2, y2], fill=(*color, 255))
    return img

def main():
    icon_dir = os.path.join(os.path.dirname(__file__), 'src-tauri', 'icons')
    os.makedirs(icon_dir, exist_ok=True)

    tray_img = draw_tray_icon(64)
    tray_img.save(os.path.join(icon_dir, 'tray-icon.png'), 'PNG')
    print("Generated tray-icon.png (64x64)")

    sizes = {
        'icon.png': 512,
        '128x128.png': 128,
        '128x128@2x.png': 256,
        '32x32.png': 32,
    }

    for filename, size in sizes.items():
        img = draw_capybara_icon(size)
        img.save(os.path.join(icon_dir, filename), 'PNG')
        print(f"Generated {filename} ({size}x{size})")

    # Generate icon.icns via iconutil
    iconset_dir = os.path.join(icon_dir, 'icon.iconset')
    os.makedirs(iconset_dir, exist_ok=True)

    icns_sizes = [16, 32, 64, 128, 256, 512, 1024]
    for s in icns_sizes:
        img = draw_capybara_icon(s)
        fname = f'icon_{s}x{s}.png'
        img.save(os.path.join(iconset_dir, fname), 'PNG')
        if s <= 512:
            img2 = draw_capybara_icon(s * 2)
            fname2 = f'icon_{s}x{s}@2x.png'
            img2.save(os.path.join(iconset_dir, fname2), 'PNG')

    icns_path = os.path.join(icon_dir, 'icon.icns')
    ret = os.system(f'iconutil -c icns "{iconset_dir}" -o "{icns_path}"')
    if ret == 0:
        print(f"Generated icon.icns")
    else:
        print("Warning: iconutil failed, ensure you're on macOS")

    import shutil
    shutil.rmtree(iconset_dir, ignore_errors=True)

if __name__ == '__main__':
    main()
