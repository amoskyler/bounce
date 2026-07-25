package ui

import (
	"encoding/binary"
	"image"
	"image/color"
	"testing"

	"github.com/alecthomas/assert/v2"
)

// sampleImage is deliberately not square and not symmetric, so that a row order
// or an axis mix up shows up as a failure rather than passing by luck.
func sampleImage() *image.NRGBA {
	img := image.NewNRGBA(image.Rect(0, 0, 3, 2))

	img.SetNRGBA(0, 0, color.NRGBA{R: 255, G: 0, B: 0, A: 255})
	img.SetNRGBA(1, 0, color.NRGBA{R: 0, G: 255, B: 0, A: 255})
	img.SetNRGBA(2, 0, color.NRGBA{R: 0, G: 0, B: 255, A: 255})
	img.SetNRGBA(0, 1, color.NRGBA{R: 255, G: 255, B: 0, A: 255})
	img.SetNRGBA(1, 1, color.NRGBA{R: 0, G: 255, B: 255, A: 255})
	img.SetNRGBA(2, 1, color.NRGBA{R: 255, G: 255, B: 255, A: 255})

	return img
}

func nrgbaAt(c color.Color) color.NRGBA {
	return color.NRGBAModel.Convert(c).(color.NRGBA)
}

func assertSameImage(t *testing.T, want image.Image, got image.Image) {
	t.Helper()

	assert.Equal(t, want.Bounds().Dx(), got.Bounds().Dx())
	assert.Equal(t, want.Bounds().Dy(), got.Bounds().Dy())

	for y := 0; y < want.Bounds().Dy(); y++ {
		for x := 0; x < want.Bounds().Dx(); x++ {
			wantR, wantG, wantB, wantA := want.At(x, y).RGBA()
			gotR, gotG, gotB, gotA := got.At(x, y).RGBA()

			assert.Equal(t, [4]uint32{wantR, wantG, wantB, wantA}, [4]uint32{gotR, gotG, gotB, gotA})
		}
	}
}

func TestDIBRoundTrip(t *testing.T) {
	want := sampleImage()

	got, err := dibToImage(imageToDIB(want))
	assert.NoError(t, err)

	assertSameImage(t, want, got)
}

// buildDIB assembles a bitmap by hand so that the decoder can be tested against
// the shapes the Windows clipboard actually produces rather than only against
// our own encoder.
func buildDIB(width, height int32, bitCount uint16, palette []color.RGBA, rows [][]byte) []byte {
	header := make([]byte, bitmapInfoHeaderSize)

	binary.LittleEndian.PutUint32(header[0:4], bitmapInfoHeaderSize)
	binary.LittleEndian.PutUint32(header[4:8], uint32(width))
	binary.LittleEndian.PutUint32(header[8:12], uint32(height))
	binary.LittleEndian.PutUint16(header[12:14], 1)
	binary.LittleEndian.PutUint16(header[14:16], bitCount)
	binary.LittleEndian.PutUint32(header[16:20], bitmapCompressionRGB)
	binary.LittleEndian.PutUint32(header[32:36], uint32(len(palette)))

	out := header
	for _, entry := range palette {
		out = append(out, entry.B, entry.G, entry.R, 0)
	}
	for _, row := range rows {
		out = append(out, row...)
	}

	return out
}

func TestDIBBottomUp24Bit(t *testing.T) {
	// 24 bit rows hold blue, green then red per pixel, are padded out to a
	// multiple of four bytes, and are stored from the bottom of the image up.
	bottom := []byte{0, 255, 255, 0, 0, 255, 0, 0}
	top := []byte{255, 0, 0, 0, 255, 0, 0, 0}

	got, err := dibToImage(buildDIB(2, 2, 24, nil, [][]byte{bottom, top}))
	assert.NoError(t, err)

	assert.Equal(t, color.NRGBA{R: 0, G: 0, B: 255, A: 255}, nrgbaAt(got.At(0, 0)))
	assert.Equal(t, color.NRGBA{R: 0, G: 255, B: 0, A: 255}, nrgbaAt(got.At(1, 0)))
	assert.Equal(t, color.NRGBA{R: 255, G: 255, B: 0, A: 255}, nrgbaAt(got.At(0, 1)))
	assert.Equal(t, color.NRGBA{R: 255, G: 0, B: 0, A: 255}, nrgbaAt(got.At(1, 1)))
}

func TestDIBTopDown(t *testing.T) {
	// A negative height means the rows are already in the order we want them.
	first := []byte{255, 0, 0, 0, 255, 0, 0, 0}
	second := []byte{0, 255, 255, 0, 0, 255, 0, 0}

	got, err := dibToImage(buildDIB(2, -2, 24, nil, [][]byte{first, second}))
	assert.NoError(t, err)

	assert.Equal(t, color.NRGBA{R: 0, G: 0, B: 255, A: 255}, nrgbaAt(got.At(0, 0)))
	assert.Equal(t, color.NRGBA{R: 255, G: 255, B: 0, A: 255}, nrgbaAt(got.At(0, 1)))
}

func TestDIBZeroAlphaTreatedAsOpaque(t *testing.T) {
	// Plenty of applications write 32 bit bitmaps with the alpha byte left at
	// zero.  Taking that literally would make the whole image invisible.
	row := []byte{
		0, 0, 255, 0,
		0, 255, 0, 0,
	}

	got, err := dibToImage(buildDIB(2, 1, 32, nil, [][]byte{row}))
	assert.NoError(t, err)

	assert.Equal(t, color.NRGBA{R: 255, G: 0, B: 0, A: 255}, nrgbaAt(got.At(0, 0)))
	assert.Equal(t, color.NRGBA{R: 0, G: 255, B: 0, A: 255}, nrgbaAt(got.At(1, 0)))
}

func TestDIBHonoursRealAlpha(t *testing.T) {
	// When any pixel sets alpha the channel is meant, and must be kept.
	row := []byte{
		0, 0, 255, 128,
		0, 255, 0, 255,
	}

	got, err := dibToImage(buildDIB(2, 1, 32, nil, [][]byte{row}))
	assert.NoError(t, err)

	assert.Equal(t, color.NRGBA{R: 255, G: 0, B: 0, A: 128}, nrgbaAt(got.At(0, 0)))
	assert.Equal(t, color.NRGBA{R: 0, G: 255, B: 0, A: 255}, nrgbaAt(got.At(1, 0)))
}

func TestDIBPalette(t *testing.T) {
	palette := []color.RGBA{
		{R: 255, G: 0, B: 0, A: 255},
		{R: 0, G: 0, B: 255, A: 255},
	}
	// 8 bit rows are padded to four bytes just like any other.
	row := []byte{1, 0, 0, 0}

	got, err := dibToImage(buildDIB(2, 1, 8, palette, [][]byte{row}))
	assert.NoError(t, err)

	assert.Equal(t, color.NRGBA{R: 0, G: 0, B: 255, A: 255}, nrgbaAt(got.At(0, 0)))
	assert.Equal(t, color.NRGBA{R: 255, G: 0, B: 0, A: 255}, nrgbaAt(got.At(1, 0)))
}

func TestDIBRejectsMalformedInput(t *testing.T) {
	tests := map[string][]byte{
		"empty":            {},
		"short header":     make([]byte, 12),
		"truncated pixels": buildDIB(4, 4, 24, nil, [][]byte{{0, 0, 0}}),
	}

	for name, data := range tests {
		t.Run(name, func(t *testing.T) {
			_, err := dibToImage(data)
			assert.Error(t, err)
		})
	}
}

func TestDIBRejectsImplausibleDimensions(t *testing.T) {
	header := make([]byte, bitmapInfoHeaderSize)

	binary.LittleEndian.PutUint32(header[0:4], bitmapInfoHeaderSize)
	binary.LittleEndian.PutUint32(header[4:8], uint32(int32(1<<20)))
	binary.LittleEndian.PutUint32(header[8:12], uint32(int32(1<<20)))
	binary.LittleEndian.PutUint16(header[14:16], 32)

	_, err := dibToImage(header)
	assert.Error(t, err)
}
