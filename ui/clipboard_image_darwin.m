//go:build darwin && !ios

#import <AppKit/AppKit.h>

#include <stddef.h>
#include <stdlib.h>
#include <string.h>

// pngFromPasteboard normalises whatever image the pasteboard is holding into
// PNG, so the go side only ever has to deal with one format.  Returns an
// autoreleased NSData, or nil when there is no image to be had.
static NSData *pngFromPasteboard(NSPasteboard *pasteboard) {
	// Screenshots and most browsers put real PNG data on the pasteboard, which
	// we can hand straight over without a re-encode.
	NSData *png = [pasteboard dataForType:NSPasteboardTypePNG];
	if (png != nil && [png length] > 0) {
		return png;
	}

	// Older applications offer TIFF, and a few offer only JPEG.  Each type is
	// named explicitly rather than asking the pasteboard for an NSImage by
	// class, which would also take in whatever else AppKit happens to be
	// willing to draw.  Asking by class would not help with a file copied in a
	// file manager either: the pasteboard carries only the url in that case,
	// and that is handled separately so the file keeps its own name and bytes.
	NSData *source = nil;
	for (NSPasteboardType type in @[ NSPasteboardTypeTIFF, @"public.jpeg" ]) {
		NSData *data = [pasteboard dataForType:type];
		if (data != nil && [data length] > 0) {
			source = data;
			break;
		}
	}
	if (source == nil) {
		return nil;
	}

	NSBitmapImageRep *bitmap = [NSBitmapImageRep imageRepWithData:source];
	if (bitmap == nil) {
		return nil;
	}

	return [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
}

// bounceClipboardImagePNG returns PNG encoded image data from the general
// pasteboard in a newly allocated buffer that the caller owns, or NULL when the
// pasteboard is not holding an image.
void *bounceClipboardImagePNG(size_t *length) {
	*length = 0;
	void *out = NULL;

	@autoreleasepool {
		NSData *png = pngFromPasteboard([NSPasteboard generalPasteboard]);
		if (png != nil && [png length] > 0) {
			NSUInteger size = [png length];
			out = malloc(size);
			if (out != NULL) {
				memcpy(out, [png bytes], size);
				*length = (size_t)size;
			}
		}
	}

	return out;
}

// bounceClipboardFilePaths returns the paths of any files on the general
// pasteboard, which is what macOS puts there when a file is copied in Finder.
// The paths are returned as count NUL terminated strings laid end to end in a
// buffer of length bytes that the caller owns.  A path may legally contain a
// newline, so a NUL is the only safe separator to use here.
char *bounceClipboardFilePaths(size_t *length, int *count) {
	*length = 0;
	*count = 0;
	char *out = NULL;

	@autoreleasepool {
		NSPasteboard *pasteboard = [NSPasteboard generalPasteboard];
		NSDictionary *options = @{NSPasteboardURLReadingFileURLsOnlyKey : @YES};
		NSArray *urls = [pasteboard readObjectsForClasses:@[ [NSURL class] ] options:options];
		if (urls == nil || [urls count] == 0) {
			return NULL;
		}

		NSMutableData *joined = [NSMutableData data];
		int found = 0;
		for (NSURL *url in urls) {
			const char *path = [[url path] fileSystemRepresentation];
			if (path == NULL) {
				continue;
			}

			[joined appendBytes:path length:strlen(path) + 1];
			found++;
		}
		if (found == 0) {
			return NULL;
		}

		NSUInteger size = [joined length];
		out = malloc(size);
		if (out != NULL) {
			memcpy(out, [joined bytes], size);
			*length = (size_t)size;
			*count = found;
		}
	}

	return out;
}

// bounceClipboardSetImagePNG places PNG encoded image data on the general
// pasteboard, returning non zero on success.  The TIFF representation is
// written alongside it because a few older applications only look for that.
int bounceClipboardSetImagePNG(const void *bytes, size_t length) {
	int ok = 0;

	@autoreleasepool {
		NSData *png = [NSData dataWithBytes:bytes length:length];
		if (png == nil || [png length] == 0) {
			return 0;
		}

		NSPasteboard *pasteboard = [NSPasteboard generalPasteboard];
		[pasteboard clearContents];

		ok = [pasteboard setData:png forType:NSPasteboardTypePNG] ? 1 : 0;

		NSBitmapImageRep *bitmap = [NSBitmapImageRep imageRepWithData:png];
		if (bitmap != nil) {
			NSData *tiff = [bitmap TIFFRepresentation];
			if (tiff != nil) {
				[pasteboard setData:tiff forType:NSPasteboardTypeTIFF];
			}
		}
	}

	return ok;
}
