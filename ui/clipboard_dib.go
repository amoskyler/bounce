package ui

import (
	"encoding/binary"
	"errors"
	"image"
	"image/color"
)

// The Windows clipboard carries images as a device independent bitmap rather
// than as a file format, so we have to unpack one by hand to get an image out
// and pack one to put an image back.  These live outside the windows build tag
// so that they can be exercised by the tests on any platform.
const (
	bitmapInfoHeaderSize = 40
	bitmapV4HeaderSize   = 108
	bitmapV5HeaderSize   = 124

	bitmapCompressionRGB       = 0
	bitmapCompressionBitFields = 3
)

type bitmapHeader struct {
	size        uint32
	width       int32
	height      int32
	bitCount    uint16
	compression uint32
	paletteSize uint32
	topDown     bool
}

func parseBitmapHeader(data []byte) (bitmapHeader, error) {
	if len(data) < bitmapInfoHeaderSize {
		return bitmapHeader{}, errors.New("bitmap is too short to hold a header")
	}

	header := bitmapHeader{
		size:        binary.LittleEndian.Uint32(data[0:4]),
		width:       int32(binary.LittleEndian.Uint32(data[4:8])),
		height:      int32(binary.LittleEndian.Uint32(data[8:12])),
		bitCount:    binary.LittleEndian.Uint16(data[14:16]),
		compression: binary.LittleEndian.Uint32(data[16:20]),
		paletteSize: binary.LittleEndian.Uint32(data[32:36]),
	}

	if header.size != bitmapInfoHeaderSize &&
		header.size != bitmapV4HeaderSize &&
		header.size != bitmapV5HeaderSize {
		return bitmapHeader{}, errors.New("unsupported bitmap header size")
	}
	if uint32(len(data)) < header.size {
		return bitmapHeader{}, errors.New("bitmap is shorter than its own header claims")
	}

	// A negative height means the rows are stored top down rather than in the
	// bottom up order bitmaps normally use.
	if header.height < 0 {
		header.topDown = true
		header.height = -header.height
	}

	if header.width <= 0 || header.height <= 0 {
		return bitmapHeader{}, errors.New("bitmap has no area")
	}

	// Guard against a header that would have us allocate an absurd image.  A
	// clipboard image larger than this is not something we could attach anyway.
	if int64(header.width)*int64(header.height) > 1<<28 {
		return bitmapHeader{}, errors.New("bitmap dimensions are implausibly large")
	}

	return header, nil
}

// dibToImage decodes a device independent bitmap of the kind the Windows
// clipboard hands out.  Only the colour depths that clipboard images are
// actually written in are handled; anything else is reported rather than
// guessed at.
func dibToImage(data []byte) (image.Image, error) {
	header, err := parseBitmapHeader(data)
	if err != nil {
		return nil, err
	}

	offset := header.size

	// With the original header the channel masks sit between the header and the
	// palette.  Later headers carry them inline, so there is nothing to skip.
	if header.compression == bitmapCompressionBitFields && header.size == bitmapInfoHeaderSize {
		offset += 12
	} else if header.compression != bitmapCompressionRGB && header.compression != bitmapCompressionBitFields {
		return nil, errors.New("compressed bitmaps are not supported")
	}

	palette := []color.RGBA{}
	if header.bitCount <= 8 {
		entries := header.paletteSize
		if entries == 0 {
			entries = 1 << header.bitCount
		}
		if uint64(offset)+uint64(entries)*4 > uint64(len(data)) {
			return nil, errors.New("bitmap palette runs past the end of the data")
		}

		for i := uint32(0); i < entries; i++ {
			entry := data[offset+i*4:]
			palette = append(palette, color.RGBA{
				B: entry[0],
				G: entry[1],
				R: entry[2],
				A: 0xff,
			})
		}
		offset += entries * 4
	}

	stride := ((int(header.width)*int(header.bitCount) + 31) / 32) * 4
	needed := int64(stride) * int64(header.height)
	if int64(len(data))-int64(offset) < needed {
		return nil, errors.New("bitmap pixel data is truncated")
	}
	pixels := data[offset:]

	img := image.NewNRGBA(image.Rect(0, 0, int(header.width), int(header.height)))

	// 32 bit clipboard bitmaps are frequently written with the alpha byte left
	// at zero, which would render the whole image invisible if taken at face
	// value.  Only honour alpha when at least one pixel actually sets it.
	hasAlpha := false
	if header.bitCount == 32 {
		for row := 0; row < int(header.height) && !hasAlpha; row++ {
			line := pixels[row*stride:]
			for x := 0; x < int(header.width); x++ {
				if line[x*4+3] != 0 {
					hasAlpha = true
					break
				}
			}
		}
	}

	for row := 0; row < int(header.height); row++ {
		line := pixels[row*stride:]

		y := row
		if !header.topDown {
			y = int(header.height) - 1 - row
		}

		for x := 0; x < int(header.width); x++ {
			var pixel color.NRGBA

			switch header.bitCount {
			case 8:
				index := int(line[x])
				if index >= len(palette) {
					return nil, errors.New("bitmap palette index out of range")
				}
				entry := palette[index]
				pixel = color.NRGBA{R: entry.R, G: entry.G, B: entry.B, A: 0xff}
			case 24:
				at := x * 3
				pixel = color.NRGBA{R: line[at+2], G: line[at+1], B: line[at], A: 0xff}
			case 32:
				at := x * 4
				alpha := uint8(0xff)
				if hasAlpha {
					alpha = line[at+3]
				}
				pixel = color.NRGBA{R: line[at+2], G: line[at+1], B: line[at], A: alpha}
			default:
				return nil, errors.New("unsupported bitmap colour depth")
			}

			img.SetNRGBA(x, y, pixel)
		}
	}

	return img, nil
}

// imageToDIB packs an image into the bottom up 32 bit device independent bitmap
// that the Windows clipboard expects, without the file header that a .bmp on
// disk would carry.
func imageToDIB(img image.Image) []byte {
	bounds := img.Bounds()
	width := bounds.Dx()
	height := bounds.Dy()

	stride := width * 4
	out := make([]byte, bitmapInfoHeaderSize+stride*height)

	binary.LittleEndian.PutUint32(out[0:4], bitmapInfoHeaderSize)
	binary.LittleEndian.PutUint32(out[4:8], uint32(int32(width)))
	binary.LittleEndian.PutUint32(out[8:12], uint32(int32(height)))
	binary.LittleEndian.PutUint16(out[12:14], 1)
	binary.LittleEndian.PutUint16(out[14:16], 32)
	binary.LittleEndian.PutUint32(out[16:20], bitmapCompressionRGB)
	binary.LittleEndian.PutUint32(out[20:24], uint32(stride*height))

	pixels := out[bitmapInfoHeaderSize:]
	for y := 0; y < height; y++ {
		line := pixels[(height-1-y)*stride:]

		for x := 0; x < width; x++ {
			pixel := color.NRGBAModel.Convert(img.At(bounds.Min.X+x, bounds.Min.Y+y)).(color.NRGBA)

			at := x * 4
			line[at] = pixel.B
			line[at+1] = pixel.G
			line[at+2] = pixel.R
			line[at+3] = pixel.A
		}
	}

	return out
}
