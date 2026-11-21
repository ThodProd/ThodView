use crate::process::{self, Result};
use image::{
    AnimationDecoder, DynamicImage, Frames, ImageDecoder, ImageFormat, ImageReader, Limits,
    RgbaImage,
};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::Path,
    time::Duration,
};

pub const EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "jif", "jfi", "webp", "avif", "heic", "heif",
    "hif", "jxl", "jp2", "j2k", "jpf", "jpx", "jpm", "j2c", "bmp", "dib", "rle", "gif", "tif",
    "tiff", "fax", "g3", "psd", "psb", "tga", "icb", "vda", "vst", "exr", "hdr", "pic", "dds",
    "ico", "cur", "pnm", "pgm", "ppm", "pbm", "pam", "pfm", "pcx", "qoi", "sgi", "rgb", "rgba",
    "bw", "ras", "sun", "xbm", "xpm", "wbmp", "wbm", "hdp", "wdp", "jxr", "svg", "svgz", "ff",
    "iff", "ilbm", "bpg", "dpx", "fits", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "dng",
    "raf", "orf", "rw2", "raw", "pef", "srw", "x3f", "3fr", "erf", "kdc", "mef", "mos", "mrw",
    "rwl", "iiq", "rwz", "dcr", "ari", "cap", "eip",
];

const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "dng", "raf", "orf", "rw2", "raw", "pef",
    "srw", "x3f", "3fr", "erf", "kdc", "mef", "mos", "mrw", "rwl", "iiq", "rwz", "dcr", "ari",
    "cap", "eip",
];

const MAX_PIXELS: u64 = 160_000_000;
const MAX_DIMENSION: u32 = 40_000;
const ANIMATION_BYTES: usize = 192 * 1024 * 1024;
const SVG_BYTES: u64 = 32 * 1024 * 1024;

pub struct Picture {
    pub frames: Vec<Frame>,
    pub plays: Option<u32>,
    pub pages: u32,
    pub note: Option<&'static str>,
}

pub struct Frame {
    pub image: RgbaImage,
    pub delay: Duration,
}

impl Picture {
    fn still(image: RgbaImage) -> Self {
        Self {
            frames: vec![Frame {
                image,
                delay: Duration::ZERO,
            }],
            plays: Some(1),
            pages: 1,
            note: None,
        }
    }
}

pub fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn jxl_header(head: &[u8]) -> bool {
    head.starts_with(b"\xff\x0a") || head.starts_with(b"\0\0\0\x0cJXL \r\n\x87\n")
}

fn svg_header(head: &[u8]) -> bool {
    head.windows(4)
        .any(|bytes| bytes.eq_ignore_ascii_case(b"<svg"))
}

pub fn is_image(path: &Path) -> bool {
    if EXTENSIONS.contains(&extension(path).as_str()) {
        return true;
    }
    let mut head = [0_u8; 512];
    let Ok(read) = File::open(path).and_then(|mut file| file.read(&mut head)) else {
        return false;
    };
    let head = &head[..read];
    image::guess_format(head).is_ok()
        || jxl_header(head)
        || svg_header(head)
        || head.starts_with(b"8BPS")
        || (head.len() > 12
            && &head[4..8] == b"ftyp"
            && [b"heic", b"heix", b"mif1", b"msf1", b"avif", b"avis"]
                .contains(&head[8..12].try_into().unwrap()))
}

fn check_size(w: u32, h: u32) -> Result<usize> {
    let pixels = u64::from(w) * u64::from(h);
    if pixels == 0 || pixels > MAX_PIXELS || w > MAX_DIMENSION || h > MAX_DIMENSION {
        return Err(format!("Изображение {w} × {h} превышает предел декодера").into());
    }
    Ok(pixels as usize)
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    limits
}

pub fn decode(path: &Path, page: u32) -> Result<Picture> {
    let path = std::path::absolute(path)?;
    let path = path.as_path();
    if page > 0 {
        return wic(path, page);
    }

    let mut head = [0_u8; 1024];
    let n = File::open(path)?.read(&mut head)?;
    let head = &head[..n];
    let extension = extension(path);
    // RAW containers can carry TIFF headers but need a camera-aware decoder.
    let raw = RAW_EXTENSIONS.contains(&extension.as_str()) && !head.starts_with(b"\xff\xd8\xff");
    if svg_header(head) || matches!(extension.as_str(), "svg" | "svgz") {
        if let Ok(image) = svg(path, head.starts_with(b"\x1f\x8b")) {
            return Ok(Picture::still(image));
        }
    }
    if jxl_header(head) {
        if let Ok(picture) = jxl(path) {
            return Ok(picture);
        }
    }
    let guessed = image::guess_format(head).ok();
    if !raw && guessed != Some(ImageFormat::Tiff) {
        if let Ok(picture) = native(path, guessed) {
            return Ok(picture);
        }
    }
    if let Ok(picture) = wic(path, 0) {
        return Ok(picture);
    }
    if raw {
        if let Ok(preview) = raw_preview(path) {
            let mut picture = Picture::still(preview);
            picture.note = Some("Встроенное превью RAW");
            return Ok(picture);
        }
    }
    if let Ok(picture) = native(path, guessed) {
        return Ok(picture);
    }
    ffmpeg(path)
        .map(Picture::still)
        .map_err(|err| format!("Не удалось открыть изображение. {err}").into())
}

fn native(path: &Path, guessed: Option<ImageFormat>) -> Result<Picture> {
    let mut reader = ImageReader::open(path)?;
    if let Some(format) = guessed {
        reader.set_format(format);
    }
    match reader.format() {
        Some(ImageFormat::Gif) => {
            let mut decoder = image::codecs::gif::GifDecoder::new(reader.into_inner())?;
            check_size(decoder.dimensions().0, decoder.dimensions().1)?;
            decoder.set_limits(limits())?;
            let repeats = AnimationDecoder::loop_count(&decoder);
            return collect_frames(decoder.into_frames(), repeats);
        }
        Some(ImageFormat::Png) => {
            let decoder =
                image::codecs::png::PngDecoder::with_limits(reader.into_inner(), limits())?;
            check_size(decoder.dimensions().0, decoder.dimensions().1)?;
            if decoder.is_apng()? {
                let animation = decoder.apng()?;
                let repeats = AnimationDecoder::loop_count(&animation);
                return collect_frames(animation.into_frames(), repeats);
            }
            return still_from_decoder(decoder);
        }
        Some(ImageFormat::WebP) => {
            let decoder = image::codecs::webp::WebPDecoder::new(reader.into_inner())?;
            check_size(decoder.dimensions().0, decoder.dimensions().1)?;
            if decoder.has_animation() {
                let repeats = AnimationDecoder::loop_count(&decoder);
                return collect_frames(decoder.into_frames(), repeats);
            }
            return still_from_decoder(decoder);
        }
        _ => {}
    }
    reader.limits(limits());
    still_from_decoder(reader.into_decoder()?)
}

fn still_from_decoder(mut decoder: impl ImageDecoder) -> Result<Picture> {
    let (w, h) = decoder.dimensions();
    check_size(w, h)?;
    let profile = decoder.icc_profile().ok().flatten();
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let mut image = image.into_rgba8();
    if let Some(profile) = profile {
        apply_rgb_profile(&mut image, &profile);
    }
    Ok(Picture::still(image))
}

fn apply_rgb_profile(image: &mut RgbaImage, profile: &[u8]) {
    use moxcms::{ColorProfile, DataColorSpace, Layout};
    if let Ok(profile) = ColorProfile::new_from_slice(profile)
        && profile.color_space == DataColorSpace::Rgb
        && let Ok(transform) = profile.create_in_place_transform_8bit(
            Layout::Rgba,
            &ColorProfile::new_srgb(),
            Default::default(),
        )
    {
        let _ = transform.transform(image);
    }
}

fn collect_frames(frames: Frames<'_>, repeats: image::metadata::LoopCount) -> Result<Picture> {
    let mut picture = Picture {
        frames: Vec::new(),
        plays: match repeats {
            image::metadata::LoopCount::Infinite => None,
            image::metadata::LoopCount::Finite(count) => Some(count.get()),
        },
        pages: 1,
        note: None,
    };
    let mut bytes = 0;
    for frame in frames {
        let frame = frame?;
        let (n, d) = frame.delay().numer_denom_ms();
        let delay = Duration::from_secs_f64((f64::from(n) / f64::from(d) / 1000.0).max(0.02));
        let image = frame.into_buffer();
        bytes += image.as_raw().len();
        if !picture.frames.is_empty() && (bytes > ANIMATION_BYTES || picture.frames.len() >= 1024) {
            picture.frames.truncate(1);
            picture.note = Some("Только первый кадр: анимация превышает лимит памяти");
            break;
        }
        picture.frames.push(Frame { image, delay });
    }
    if picture.frames.is_empty() {
        return Err("В файле нет кадров".into());
    }
    Ok(picture)
}

fn jxl(path: &Path) -> Result<Picture> {
    use jxl_oxide::PixelFormat::*;
    let mut source = jxl_oxide::JxlImage::builder().open(path)?;
    check_size(source.width(), source.height())?;
    if !source.pixel_format().is_grayscale() {
        source.request_color_encoding(jxl_oxide::EnumColourEncoding::srgb(
            jxl_oxide::RenderingIntent::Relative,
        ));
    }
    let mut picture = Picture {
        frames: Vec::new(),
        plays: Some(1),
        pages: 1,
        note: None,
    };
    let clocks = source.image_header().metadata.animation.as_ref();
    if let Some(clock) = clocks {
        picture.plays = (clock.num_loops != 0).then_some(clock.num_loops);
    }
    let mut bytes = 0;
    for index in 0..source.num_loaded_keyframes() {
        let rendered = source.render_frame(index)?;
        let mut stream = rendered.stream();
        let (w, h) = (stream.width(), stream.height());
        let count = check_size(w, h)?;
        bytes += count * 4;
        if index > 0 && (bytes > ANIMATION_BYTES || index >= 1024) {
            picture.frames.truncate(1);
            picture.note = Some("Только первый кадр: анимация превышает лимит памяти");
            break;
        }
        let format = source.pixel_format();
        let rgba = if format == Rgba {
            let mut rgba = vec![0; count * 4];
            stream.write_to_buffer(&mut rgba);
            rgba
        } else {
            let mut samples = vec![0; count * format.channels()];
            stream.write_to_buffer(&mut samples);
            let mut rgba = Vec::with_capacity(count * 4);
            for pixel in samples.chunks_exact(format.channels()) {
                let converted = match format {
                    Gray => [pixel[0], pixel[0], pixel[0], 255],
                    Graya => [pixel[0], pixel[0], pixel[0], pixel[1]],
                    Rgb => [pixel[0], pixel[1], pixel[2], 255],
                    _ => return Err("Не удалось преобразовать цвета JPEG XL".into()),
                };
                rgba.extend_from_slice(&converted);
            }
            rgba
        };
        let seconds = clocks.map_or(0.0, |clock| {
            f64::from(rendered.duration()) * f64::from(clock.tps_denominator)
                / f64::from(clock.tps_numerator.max(1))
        });
        picture.frames.push(Frame {
            image: RgbaImage::from_raw(w, h, rgba).ok_or("Некорректное изображение JPEG XL")?,
            delay: Duration::from_secs_f64(seconds.clamp(0.02, 86_400.0)),
        });
    }
    if picture.frames.is_empty() {
        return Err("В файле JPEG XL нет кадров".into());
    }
    Ok(picture)
}

fn svg(path: &Path, compressed: bool) -> Result<RgbaImage> {
    use resvg::{tiny_skia, usvg};
    let mut data = Vec::new();
    let file = File::open(path)?;
    if compressed {
        flate2::read::GzDecoder::new(file)
            .take(SVG_BYTES + 1)
            .read_to_end(&mut data)?;
    } else {
        file.take(SVG_BYTES + 1).read_to_end(&mut data)?;
    }
    if data.len() as u64 > SVG_BYTES {
        return Err("Превышен предел размера SVG".into());
    }
    let mut options = usvg::Options::default();
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(&data, &options)?;
    let size = tree.size();
    let scale = (4096.0 / size.width().max(size.height())).min(2.0);
    let w = (size.width() * scale).ceil() as u32;
    let h = (size.height() * scale).ceil() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("Некорректный размер SVG")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut data = pixmap.take();
    for pixel in data.chunks_exact_mut(4) {
        let a = u32::from(pixel[3]);
        if a > 0 && a < 255 {
            for channel in &mut pixel[..3] {
                *channel = (u32::from(*channel) * 255 / a).min(255) as u8;
            }
        }
    }
    RgbaImage::from_raw(w, h, data).ok_or_else(|| "Некорректное изображение SVG".into())
}

fn wic(path: &Path, index: u32) -> Result<Picture> {
    use windows::Win32::{
        Foundation::GENERIC_READ,
        Graphics::Imaging::*,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    };
    let _com = crate::shell::Com::new()?;
    // WIC objects and the filename remain alive throughout conversion.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let filename = crate::shell::wide(path.as_os_str());
        let decoder = factory.CreateDecoderFromFilename(
            windows::core::PCWSTR(filename.as_ptr()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )?;
        let frame = decoder.GetFrame(index)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let (mut w, mut h) = (0, 0);
        converter.GetSize(&mut w, &mut h)?;
        let mut rgba = vec![0; check_size(w, h)? * 4];
        converter.CopyPixels(std::ptr::null(), w * 4, &mut rgba)?;
        let mut picture =
            Picture::still(RgbaImage::from_raw(w, h, rgba).ok_or("Некорректное изображение WIC")?);
        // Read TIFF EXIF orientation through the native decoder when available.
        if let Ok(reader) = ImageReader::open(path)?.with_guessed_format() {
            if let Ok(mut native) = reader.into_decoder() {
                if index == 0 {
                    if let Ok(orientation) = native.orientation() {
                        let frame = &mut picture.frames[0];
                        let mut rotated =
                            DynamicImage::ImageRgba8(std::mem::take(&mut frame.image));
                        rotated.apply_orientation(orientation);
                        frame.image = rotated.into_rgba8();
                    }
                }
            }
        }
        picture.pages = decoder.GetFrameCount()?;
        Ok(picture)
    }
}

fn ffmpeg(path: &Path) -> Result<RgbaImage> {
    let mut args = process::args(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-protocol_whitelist",
        "file,pipe",
        "-max_pixels",
        "160000000",
        "-i",
    ]);
    args.push(path.into());
    args.extend(process::args(&[
        "-map",
        "0:v:0",
        "-frames:v",
        "1",
        "-vf",
        "scale=w='min(iw,40000)':h='min(ih,40000)':force_original_aspect_ratio=decrease",
        "-pix_fmt",
        "rgba",
        "-c:v",
        "pam",
        "-f",
        "image2pipe",
        "pipe:1",
    ]));
    let data = process::run(&process::ffmpeg(), &args, MAX_PIXELS as usize * 4 + 4096)?;
    // PAM transfers uncompressed straight RGBA. Avoid an intermediate PNG encode/decode.
    let end = data
        .get(..data.len().min(4096))
        .unwrap()
        .windows(7)
        .position(|part| part == b"ENDHDR\n")
        .ok_or("Нет кадра FFmpeg")?;
    let header = std::str::from_utf8(&data[..end])?;
    let value = |key: &str| -> Option<u32> {
        header
            .lines()
            .find_map(|line| line.strip_prefix(key)?.trim().parse().ok())
    };
    let (w, h) = (
        value("WIDTH ").ok_or("Нет ширины кадра")?,
        value("HEIGHT ").ok_or("Нет высоты кадра")?,
    );
    let count = check_size(w, h)?;
    if value("DEPTH ") != Some(4) || data.len() != end + 7 + count * 4 {
        return Err("Некорректный кадр FFmpeg".into());
    }
    let mut data = data;
    data.copy_within(end + 7.., 0);
    data.truncate(count * 4);
    RgbaImage::from_raw(w, h, data).ok_or_else(|| "Некорректное изображение FFmpeg".into())
}

fn raw_preview(path: &Path) -> Result<RgbaImage> {
    for tag in ["-JpgFromRaw", "-PreviewImage", "-OtherImage"] {
        let data = crate::metadata::invoke(
            path,
            &[tag.to_owned(), "-b".to_owned()],
            MAX_PIXELS as usize,
        )?;
        if data.is_empty() {
            continue;
        }
        let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format()?;
        reader.limits(limits());
        if let Ok(picture) = reader
            .into_decoder()
            .map_err(Into::into)
            .and_then(still_from_decoder)
        {
            return Ok(picture.frames.into_iter().next().unwrap().image);
        }
    }
    Err("Встроенное превью RAW отсутствует".into())
}

pub fn export(image: &RgbaImage, path: &Path) -> Result<()> {
    let folder = path.parent().ok_or("Не найдена папка назначения")?;
    let mut temp = tempfile::NamedTempFile::new_in(folder)?;
    match extension(path).as_str() {
        "jpg" | "jpeg" => {
            let rgb = image::RgbImage::from_fn(image.width(), image.height(), |x, y| {
                let [r, g, b, a] = image.get_pixel(x, y).0.map(u32::from);
                image::Rgb(
                    [r, g, b].map(|channel| ((channel * a + 255 * (255 - a) + 127) / 255) as u8),
                )
            });
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut temp, 94).encode_image(&rgb)?;
        }
        ext => {
            let format = match ext {
                "png" => ImageFormat::Png,
                "webp" => ImageFormat::WebP,
                "bmp" => ImageFormat::Bmp,
                "tif" | "tiff" => ImageFormat::Tiff,
                "qoi" => ImageFormat::Qoi,
                _ => return Err(format!("Формат сохранения .{ext} не поддерживается").into()),
            };
            image::write_buffer_with_format(
                &mut temp,
                image.as_raw(),
                image.width(),
                image.height(),
                image::ColorType::Rgba8,
                format,
            )?;
        }
    }
    temp.flush()?;
    temp.persist(path)?;
    Ok(())
}
