// macOS-only bridge. No Swift runtime, external helper or image file roundtrip.
#import <Foundation/Foundation.h>
#import <CoreImage/CoreImage.h>
#import <CoreGraphics/CoreGraphics.h>
#import <ImageIO/ImageIO.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <math.h>
#include <dlfcn.h>

typedef struct {
    float *pixels; // RGBA float32, top row first, malloc-owned until release.
    uint32_t width, height, monochrome, lens_correction_supported;
    // The file's EXIF orientation (1-8), and whether the pixels have it applied.
    uint32_t orientation, oriented;
    // The size a whole read would have, in the pixels' own orientation; the
    // same as width and height unless the read was reduced.
    uint32_t full_width, full_height;
    // The generation decoded with (9 or 8), 0 if the file failed before one
    // was chosen. With `decoder_major` 0 and no RAW 9 for this file, `declined`
    // says why, as asking for 9 alone would have failed.
    int32_t major;
    char declined[768];
    char version[32];
    char error[768];
} SFAppleRaw;

static int fail(SFAppleRaw *out, int code, NSString *message) {
    snprintf(out->error, sizeof(out->error), "%s", message.UTF8String ?: "Unknown Apple RAW error");
    return code;
}

// Some valid tiny/custom DNGs initialize a filter but expose no supported
// sensor image (1: let the portable backend handle them). Bounds are checked
// before conversion, arithmetic, or allocation.
static int check_bounds(CGRect bounds, SFAppleRaw *out) {
    if (CGRectIsEmpty(bounds))
        return fail(out, 1, @"Apple RAW does not support this sensor image geometry");
    if (!isfinite(bounds.origin.x) || !isfinite(bounds.origin.y)
        || !isfinite(bounds.size.width) || !isfinite(bounds.size.height)
        || bounds.size.width < 1 || bounds.size.height < 1
        || bounds.size.width != floor(bounds.size.width) || bounds.size.height != floor(bounds.size.height)
        || bounds.size.width > 100000 || bounds.size.height > 100000
        || bounds.size.width * bounds.size.height > 200000000)
        return fail(out, 2, @"Invalid or excessive Apple RAW output dimensions (maximum 200 MP)");
    return 0;
}

// Core Image's identifiers for one decoder generation. Version 9's constants
// don't exist before macOS 15.4-era Core Image and carry no availability
// annotation, so a direct reference would be a strong import that stops the
// helper loading on older systems. Look them up instead: a missing symbol just
// means this macOS has no RAW 9, and the caller moves on to RAW 8.
static NSArray<CIRAWDecoderVersion> *decoder_versions(int major) {
    if (major != 9) return @[CIRAWDecoderVersion8, CIRAWDecoderVersion8DNG];
    NSMutableArray<CIRAWDecoderVersion> *found = [NSMutableArray array];
    const char *names[] = {"CIRAWDecoderVersion9", "CIRAWDecoderVersion9DNG"};
    for (size_t i = 0; i < 2; i++) {
        CIRAWDecoderVersion __unsafe_unretained *symbol =
            (CIRAWDecoderVersion __unsafe_unretained *)dlsym(RTLD_DEFAULT, names[i]);
        if (symbol && *symbol) [found addObject:*symbol];
    }
    return found;
}

// 0 success, 1 incompatible decoder, 2 fatal I/O/resource failure. The caller always frees
// pixels, including on exceptions. No exception crosses the C ABI.
// `defer_orientation` leaves a file Apple cannot lens-correct unoriented, so the
// caller can correct it in sensor coordinates first and orient it afterwards.
// `decoder_major` (9 or 8) selects that Core Image decoder generation; 0 takes 9
// if this file's filter offers it, else 8, from the one filter (no second open
// of the file just to find 9 missing).
// `max_long_side`, when not 0, has Apple develop no larger than that on the long
// side, through the same processing, which is far quicker for a small copy. A file
// Apple can't lens-correct while Negs has a profile for it (`own_lens_profile`) is
// read whole, since Negs corrects it in sensor pixels.
int sf_apple_raw_decode(const char *path, int decoder_major, int orient, int as_shot,
                        int lens_correction, int defer_orientation, uint32_t max_long_side,
                        int own_lens_profile, SFAppleRaw *out) {
    @autoreleasepool {
        @try {
            NSURL *url = CFBridgingRelease(CFURLCreateFromFileSystemRepresentation(
                kCFAllocatorDefault, (const UInt8 *)path, strlen(path), false));
            if (!url || ![[NSFileManager defaultManager] isReadableFileAtPath:url.path])
                return fail(out, 2, @"Cannot read RAW file");
            CIRAWFilter *raw = [CIRAWFilter filterWithImageURL:url];
            if (!raw) return fail(out, 1, @"Apple RAW does not support this file on this macOS version");
            // Query this file, not an extension/camera allowlist. DNG uses its
            // own version identifiers, selected from the filter's actual list.
            int major = decoder_major ? decoder_major : 9;
            CIRAWDecoderVersion selected = nil;
            for (CIRAWDecoderVersion candidate in decoder_versions(major)) {
                if ([raw.supportedDecoderVersions containsObject:candidate]) {
                    selected = candidate;
                    break;
                }
            }
            if (!selected && decoder_major == 0) {
                snprintf(out->declined, sizeof(out->declined), "%s", [NSString stringWithFormat:
                    @"Apple RAW %d is not supported for this file (available: %@)",
                    major, [raw.supportedDecoderVersions componentsJoinedByString:@", "]].UTF8String ?: "Unknown Apple RAW error");
                major = 8;
                for (CIRAWDecoderVersion candidate in decoder_versions(major)) {
                    if ([raw.supportedDecoderVersions containsObject:candidate]) {
                        selected = candidate;
                        break;
                    }
                }
            }
            out->major = major;
            if (!selected) return fail(out, 1, [NSString stringWithFormat:
                @"Apple RAW %d is not supported for this file (available: %@)",
                major, [raw.supportedDecoderVersions componentsJoinedByString:@", "]]);
            raw.decoderVersion = selected;
            if (![raw.decoderVersion isEqualToString:selected])
                return fail(out, 1, @"Apple RAW did not accept the requested decoder version");
            raw.draftModeEnabled = NO;
            raw.scaleFactor = 1;
            out->orientation = (uint32_t)raw.orientation;
            if (out->orientation < 1 || out->orientation > 8) out->orientation = 1;
            if (defer_orientation && !raw.isLensCorrectionSupported) orient = 0;
            out->oriented = orient && out->orientation != 1;
            if (!orient) raw.orientation = kCGImagePropertyOrientationUp;
            // Explicit scene-linear development: no automatic photographic look.
            raw.exposure = 0;
            raw.baselineExposure = 0;
            raw.shadowBias = 0;
            raw.boostAmount = 0;
            raw.boostShadowAmount = 0;
            raw.gamutMappingEnabled = NO;
            raw.extendedDynamicRangeAmount = 0;
            if (raw.isLocalToneMapSupported) raw.localToneMapAmount = 0;
            // The vendor's distortion and vignetting corrections, applied
            // together and scaled to fill the same extent. Off also leaves a
            // DNG's own warp/vignette opcodes unapplied.
            out->lens_correction_supported = raw.isLensCorrectionSupported;
            if (raw.isLensCorrectionSupported) raw.lensCorrectionEnabled = lens_correction ? YES : NO;
            if (raw.isLuminanceNoiseReductionSupported) raw.luminanceNoiseReductionAmount = 0;
            if (raw.isColorNoiseReductionSupported) raw.colorNoiseReductionAmount = 0;
            if (raw.isSharpnessSupported) raw.sharpnessAmount = 0;
            if (raw.isContrastSupported) raw.contrastAmount = 0;
            if (raw.isDetailSupported) raw.detailAmount = 0;
            if (raw.isMoireReductionSupported) raw.moireReductionAmount = 0;
            // Selector checks avoid a dependency on a new OS availability
            // runtime when the Rust caller was built with an older toolchain.
            if ([raw respondsToSelector:NSSelectorFromString(@"isHighlightRecoverySupported")]
                && [[raw valueForKey:@"highlightRecoverySupported"] boolValue])
                [raw setValue:@YES forKey:@"highlightRecoveryEnabled"];
            NSDictionary *properties = raw.properties;
            NSDictionary *tiff = properties[(__bridge NSString *)kCGImagePropertyTIFFDictionary];
            // BlackIsZero marks a monochrome DNG. Olympus/OM ORFs tag their
            // colour CFA image the same way, so trust it only in a DNG.
            out->monochrome = [properties[(__bridge NSString *)kCGImagePropertyColorModel]
                                isEqual:(__bridge NSString *)kCGImagePropertyColorModelGray]
                || (properties[(__bridge NSString *)kCGImagePropertyDNGDictionary] != nil
                    && [tiff[(__bridge NSString *)kCGImagePropertyTIFFPhotometricInterpretation] intValue] == 1);
            // Other modes develop at D65, then use the same CAT02/ACES tint
            // operation as rawler. Apple's unrelated -150..150 tint is unused.
            if (!as_shot && !out->monochrome) raw.neutralChromaticity = CGPointMake(0.3127, 0.3290);
            snprintf(out->version, sizeof(out->version), "%s", raw.decoderVersion.UTF8String);
            CIImage *image = raw.outputImage;
            if (!image) return fail(out, 1, @"Apple RAW produced no image for this file");
            CGRect bounds = image.extent;
            int status = check_bounds(bounds, out);
            if (status) return status;
            out->full_width = (uint32_t)bounds.size.width;
            out->full_height = (uint32_t)bounds.size.height;
            uint32_t long_side = MAX(out->full_width, out->full_height);
            if (max_long_side > 0 && long_side > max_long_side
                && !(own_lens_profile && !raw.isLensCorrectionSupported)) {
                raw.scaleFactor = (double)max_long_side / long_side;
                image = raw.outputImage;
                if (!image) return fail(out, 1, @"Apple RAW produced no reduced image for this file");
                // A scaled extent can fall between pixels.
                bounds = CGRectIntegral(image.extent);
                if ((status = check_bounds(bounds, out))) return status;
            }
            out->width = (uint32_t)bounds.size.width;
            out->height = (uint32_t)bounds.size.height;
            size_t row_bytes = (size_t)out->width * 4 * sizeof(float);
            out->pixels = calloc(out->height, row_bytes);
            if (!out->pixels) return fail(out, 2, @"Cannot allocate Apple RAW output");
            static CIContext *context;
            static CGColorSpaceRef linear;
            static dispatch_once_t once;
            dispatch_once(&once, ^{
                linear = CGColorSpaceCreateWithName(kCGColorSpaceExtendedLinearSRGB);
                context = [CIContext contextWithOptions:@{
                    kCIContextWorkingColorSpace: (__bridge id)linear,
                    kCIContextWorkingFormat: @(kCIFormatRGBAf),
                    kCIContextCacheIntermediates: @NO
                }];
            });
            CIRenderDestination *destination = [[CIRenderDestination alloc]
                initWithBitmapData:out->pixels width:out->width height:out->height
                bytesPerRow:row_bytes format:kCIFormatRGBAf];
            destination.colorSpace = linear;
            destination.alphaMode = CIRenderDestinationAlphaNone;
            destination.flipped = YES;
            destination.clamped = NO;
            destination.dithered = NO;
            NSError *error = nil;
            CIRenderTask *task = [context startTaskToRender:image fromRect:bounds
                toDestination:destination atPoint:CGPointZero error:&error];
            if (!task || ![task waitUntilCompletedAndReturnError:&error])
                return fail(out, 1, error.localizedDescription ?: @"Apple RAW rendering failed");
            return 0;
        } @catch (NSException *exception) {
            return fail(out, 1, exception.reason ?: @"Apple RAW raised an exception");
        }
    }
}

void sf_apple_raw_free(float *pixels) { free(pixels); }
