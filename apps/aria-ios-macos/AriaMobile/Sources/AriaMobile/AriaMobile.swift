import CoreGraphics
import Foundation
import AriaMobileFFI

/// Swift front door to the Aria engine.
///
/// Mirrors the Android `AriaNative` bridge: render a frame on the device GPU
/// and get pixels back — here as a `CGImage` ready for SwiftUI.
public enum AriaMobile {
    /// Render the built-in demo scene at `width`×`height` pixels.
    ///
    /// Returns `nil` if the GPU device/compositor could not be created. This is
    /// CPU-bound + GPU work, so call it off the main thread.
    public static func renderDemo(width: Int, height: Int) -> CGImage? {
        makeImage(aria_render_demo(UInt32(width), UInt32(height)))
    }

    /// Decode + composite the first frame of the video at `path`, scaled to fit
    /// `maxWidth`×`maxHeight`. Returns `nil` on failure (bad path, decode error,
    /// no GPU). Call off the main thread.
    public static func renderFileFrame(path: String, maxWidth: Int, maxHeight: Int) -> CGImage? {
        let bytes = Array(path.utf8)
        let image = bytes.withUnsafeBufferPointer { buffer in
            aria_render_file_frame(
                buffer.baseAddress, buffer.count, UInt32(maxWidth), UInt32(maxHeight)
            )
        }
        return makeImage(image)
    }

    /// Decode + composite the first frame of the demo clip bundled with this
    /// package (`sample.mp4`). Exercises the platform hardware decoder.
    public static func renderBundledSampleFrame(maxWidth: Int, maxHeight: Int) -> CGImage? {
        guard let url = Bundle.module.url(forResource: "sample", withExtension: "mp4") else {
            return nil
        }
        return renderFileFrame(path: url.path, maxWidth: maxWidth, maxHeight: maxHeight)
    }

    /// Wrap a native RGBA buffer in a `CGImage` without copying: the data
    /// provider takes ownership and releases the buffer through
    /// `aria_image_free` once CoreGraphics is done with it (any thread —
    /// the native free is thread-safe). Safe to call with a failed (null)
    /// image — returns `nil`.
    static func makeImage(_ image: AriaImage) -> CGImage? {
        let w = Int(image.width)
        let h = Int(image.height)
        guard let data = image.data, image.len > 0, w > 0, h > 0, image.len == w * h * 4 else {
            aria_image_free(image)
            return nil
        }

        // The release callback is a C function pointer and can't capture, so
        // the original struct rides along in a heap box as the provider's
        // `dataInfo` and is handed back to `aria_image_free` verbatim.
        let owner = UnsafeMutablePointer<AriaImage>.allocate(capacity: 1)
        owner.initialize(to: image)
        let release: CGDataProviderReleaseDataCallback = { info, _, _ in
            guard let owner = info?.assumingMemoryBound(to: AriaImage.self) else { return }
            aria_image_free(owner.pointee)
            owner.deinitialize(count: 1)
            owner.deallocate()
        }
        guard let provider = CGDataProvider(
            dataInfo: owner,
            data: data,
            size: image.len,
            releaseData: release
        ) else {
            owner.deinitialize(count: 1)
            owner.deallocate()
            aria_image_free(image)
            return nil
        }

        let bitmapInfo = CGBitmapInfo(
            rawValue: CGImageAlphaInfo.premultipliedLast.rawValue
        )
        return CGImage(
            width: w,
            height: h,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: w * 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: bitmapInfo,
            provider: provider,
            decode: nil,
            shouldInterpolate: false,
            intent: .defaultIntent
        )
    }
}

/// A live preview session for interactive scrubbing.
///
/// Wraps the native `AriaPreview` handle (a persistent GPU device + decoder
/// cache bound to a project), so each `frame(atSeconds:)` only pays for that
/// frame. Not thread-safe: render off the main thread and serialize calls on a
/// single instance (a scrubber issues them one at a time).
public final class AriaPreview {
    private let handle: OpaquePointer

    private init(handle: OpaquePointer) {
        self.handle = handle
    }

    /// Open the synthetic, file-free scrub demo (a hue sweep over ~6s).
    /// `nil` if the GPU/renderer could not be brought up.
    public static func demo() -> AriaPreview? {
        guard let handle = aria_preview_open_demo() else { return nil }
        return AriaPreview(handle: handle)
    }

    /// Open a preview that scrubs the video file at `path`. `nil` on failure.
    public static func video(path: String) -> AriaPreview? {
        let bytes = Array(path.utf8)
        let handle = bytes.withUnsafeBufferPointer { buffer in
            aria_preview_open_video(buffer.baseAddress, buffer.count)
        }
        guard let handle else { return nil }
        return AriaPreview(handle: handle)
    }

    /// Open a preview for the demo clip bundled with this package
    /// (`sample.mp4`). Exercises the platform hardware decoder while scrubbing.
    public static func bundledSample() -> AriaPreview? {
        guard let url = Bundle.module.url(forResource: "sample", withExtension: "mp4") else {
            return nil
        }
        return video(path: url.path)
    }

    /// Total scrub length in seconds.
    public var durationSeconds: Double {
        aria_preview_duration_seconds(handle)
    }

    /// Render the frame at `seconds` (clamped to range). Call off the main
    /// thread; do not call concurrently on the same instance.
    public func frame(atSeconds seconds: Double) -> CGImage? {
        AriaMobile.makeImage(aria_preview_render(handle, seconds))
    }

    deinit {
        aria_preview_close(handle)
    }
}
