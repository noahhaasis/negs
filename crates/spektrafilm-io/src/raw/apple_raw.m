// macOS-only bridge. No Swift runtime, external helper or image file roundtrip.
#import <Foundation/Foundation.h>
#import <CoreImage/CoreImage.h>
#import <CoreGraphics/CoreGraphics.h>
#import <ImageIO/ImageIO.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <math.h>

typedef struct {
    float *pixels; // RGBA float32, top row first, malloc-owned until release.
    uint32_t width, height, monochrome;
    char version[32];
    char error[768];
} SFAppleRaw;

static int fail(SFAppleRaw *out, int code, NSString *message) {
    snprintf(out->error, sizeof(out->error), "%s", message.UTF8String ?: "Unknown Apple RAW error");
    return code;
}

// 0 success, 1 unsupported format, 2 I/O/render failure. The caller always frees
// pixels, including on exceptions. No exception crosses the C ABI.
int sf_apple_raw_decode(const char *path, int orient, int as_shot, SFAppleRaw *out) {
    @autoreleasepool {
        @try {
            NSURL *url = CFBridgingRelease(CFURLCreateFromFileSystemRepresentation(
                kCFAllocatorDefault, (const UInt8 *)path, strlen(path), false));
            if (!url || ![[NSFileManager defaultManager] isReadableFileAtPath:url.path])
                return fail(out, 2, @"Cannot read RAW file");
            CIRAWFilter *raw = [CIRAWFilter filterWithImageURL:url];
            if (!raw) return fail(out, 1, @"Apple RAW does not support this file on this macOS version");
            raw.draftModeEnabled = NO;
            raw.scaleFactor = 1;
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
            if (raw.isLensCorrectionSupported) raw.lensCorrectionEnabled = NO;
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
            out->monochrome = [properties[(__bridge NSString *)kCGImagePropertyColorModel]
                                isEqual:(__bridge NSString *)kCGImagePropertyColorModelGray]
                || [tiff[(__bridge NSString *)kCGImagePropertyTIFFPhotometricInterpretation] intValue] == 1;
            // Other modes develop at D65, then use the same CAT02/ACES tint
            // operation as rawler. Apple's unrelated -150..150 tint is unused.
            if (!as_shot && !out->monochrome) raw.neutralChromaticity = CGPointMake(0.3127, 0.3290);
            snprintf(out->version, sizeof(out->version), "%s", raw.decoderVersion.UTF8String);
            CIImage *image = raw.outputImage;
            if (!image) return fail(out, 1, @"Apple RAW produced no image for this file");
            CGRect bounds = image.extent;
            // Some valid tiny/custom DNGs initialize a filter but expose no
            // supported sensor image. Let the portable backend handle them.
            if (CGRectIsEmpty(bounds))
                return fail(out, 1, @"Apple RAW does not support this sensor image geometry");
            // Bounds are checked before conversion, arithmetic, or allocation.
            if (!isfinite(bounds.origin.x) || !isfinite(bounds.origin.y)
                || !isfinite(bounds.size.width) || !isfinite(bounds.size.height)
                || bounds.size.width < 1 || bounds.size.height < 1
                || bounds.size.width != floor(bounds.size.width) || bounds.size.height != floor(bounds.size.height)
                || bounds.size.width > 100000 || bounds.size.height > 100000
                || bounds.size.width * bounds.size.height > 200000000)
                return fail(out, 2, @"Invalid or excessive Apple RAW output dimensions (maximum 200 MP)");
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
                return fail(out, 2, error.localizedDescription ?: @"Apple RAW rendering failed");
            return 0;
        } @catch (NSException *exception) {
            return fail(out, 2, exception.reason ?: @"Apple RAW raised an exception");
        }
    }
}

void sf_apple_raw_free(float *pixels) { free(pixels); }
