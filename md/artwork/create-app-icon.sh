#!/bin/bash

# Create App Icon from SVG with rounded rectangle background
# Usage: ./create-app-icon.sh

set -e

LOGO_SVG="./logo-alcove.svg"
ICON_NAME="AppIcon"
OUTPUT_DIR="../symposium/macos-app"
TEMP_DIR="/tmp/symposium-icon-$$"

# Check if logo.svg exists
if [ ! -f "$LOGO_SVG" ]; then
    echo "Error: logo.svg not found in current directory"
    exit 1
fi

# Create temporary directory and copy logo
mkdir -p "$TEMP_DIR"
cp "$LOGO_SVG" "$TEMP_DIR/logo.svg"
cd "$TEMP_DIR"

echo "Creating app icon from $LOGO_SVG..."

# Function to create icon at specific size
create_icon() {
    local size=$1
    local filename=$2
    
    # Create rounded rectangle background with macOS-style appearance
    cat > background.svg << EOF
<svg xmlns="http://www.w3.org/2000/svg" width="$size" height="$size" viewBox="0 0 $size $size">
  <defs>
    <linearGradient id="bg" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" style="stop-color:#f8f9fa;stop-opacity:1" />
      <stop offset="100%" style="stop-color:#e9ecef;stop-opacity:1" />
    </linearGradient>
  </defs>
  <rect width="$size" height="$size" rx="$(echo "$size * 0.175" | bc)" ry="$(echo "$size * 0.175" | bc)" fill="url(#bg)" stroke="#dee2e6" stroke-width="1"/>
</svg>
EOF

    # Convert background to PNG
    if command -v rsvg-convert > /dev/null; then
        rsvg-convert -w $size -h $size background.svg > background.png
    elif command -v cairosvg > /dev/null; then
        cairosvg -W $size -H $size background.svg -o background.png
    else
        echo "Error: Need rsvg-convert (librsvg) or cairosvg to convert SVG"
        echo "Install with: brew install librsvg"
        exit 1
    fi

    # Convert logo to PNG at smaller size (80% of icon to leave margin)
    local logo_size=$(echo "$size * 0.8" | bc | cut -d. -f1)
    local offset=$(echo "($size - $logo_size) / 2" | bc | cut -d. -f1)
    
    if command -v rsvg-convert > /dev/null; then
        rsvg-convert -w $logo_size -h $logo_size logo.svg > logo.png
    else
        cairosvg -W $logo_size -H $logo_size logo.svg -o logo.png
    fi

    # Composite logo onto background
    if command -v convert > /dev/null; then
        convert background.png logo.png -geometry +$offset+$offset -composite "$filename"
    else
        echo "Error: Need ImageMagick (convert command)"
        echo "Install with: brew install imagemagick"
        exit 1
    fi
}

# Create all required icon sizes
echo "Generating icon sizes..."
create_icon 1024 "icon_512x512@2x.png"
create_icon 512 "icon_512x512.png"
create_icon 512 "icon_256x256@2x.png"
create_icon 256 "icon_256x256.png"
create_icon 256 "icon_128x128@2x.png"
create_icon 128 "icon_128x128.png"
create_icon 64 "icon_32x32@2x.png"
create_icon 32 "icon_32x32.png"
create_icon 32 "icon_16x16@2x.png"
create_icon 16 "icon_16x16.png"

# Create iconset directory
mkdir -p "$ICON_NAME.iconset"
mv *.png "$ICON_NAME.iconset/"

# Create .icns file
echo "Creating $ICON_NAME.icns..."
iconutil -c icns "$ICON_NAME.iconset"

# Move to final location
mv "$ICON_NAME.icns" "$OUTPUT_DIR/"

# Cleanup
cd /
rm -rf "$TEMP_DIR"

echo "✅ App icon created: $OUTPUT_DIR/$ICON_NAME.icns"
echo "Next steps:"
echo "1. Update Info.plist to reference the icon"
echo "2. Rebuild the app to see the new icon"