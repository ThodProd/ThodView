use crate::process::Result;
use eframe::egui::{Rect, pos2, vec2};
use image::RgbaImage;
use windows::{
    Globalization::Language,
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Security::Cryptography::CryptographicBuffer,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::HSTRING,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum OcrLanguage {
    #[default]
    Auto,
    Russian,
    English,
}

impl OcrLanguage {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Russian, Self::English];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Авто",
            Self::Russian => "Русский",
            Self::English => "English",
        }
    }

    fn engine(self) -> Result<OcrEngine> {
        if self == Self::Auto {
            if let Ok(engine) = OcrEngine::TryCreateFromUserProfileLanguages() {
                return Ok(engine);
            }
            return Self::Russian.engine().or_else(|_| Self::English.engine());
        }

        let tag = if self == Self::Russian { "ru" } else { "en" };
        let language = Language::CreateLanguage(&HSTRING::from(tag))?;
        if !OcrEngine::IsLanguageSupported(&language)? {
            return Err(format!(
                "Установите компонент OCR Windows для языка «{}»",
                self.label()
            )
            .into());
        }
        Ok(OcrEngine::TryCreateFromLanguage(&language)?)
    }
}

pub struct Word {
    // Fractional coordinates relative to the image, independent of zoom and OCR input size.
    pub bounds: Rect,
    pub text: String,
}

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        // The worker that initialized the apartment also releases it.
        unsafe { RoUninitialize() };
    }
}

pub fn recognize(image: RgbaImage, language: OcrLanguage) -> Result<Vec<Vec<Word>>> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let _apartment = Apartment;
    let engine = language.engine()?;
    let max_side = OcrEngine::MaxImageDimension()?;
    let gray = image::ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
        let [r, g, b, a] = image.get_pixel(x, y).0.map(u32::from);
        let luma = (r * 299 + g * 587 + b * 114) / 1000;
        image::Luma([((luma * a + 255 * (255 - a)) / 255) as u8])
    });
    let gray = if gray.width().max(gray.height()) > max_side {
        image::DynamicImage::ImageLuma8(gray)
            .resize(max_side, max_side, image::imageops::FilterType::Triangle)
            .into_luma8()
    } else {
        gray
    };
    let (width, height) = gray.dimensions();
    let buffer = CryptographicBuffer::CreateFromByteArray(gray.as_raw())?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Gray8,
        width as i32,
        height as i32,
    )?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let lines = result.Lines()?;
    let mut recognized = Vec::new();
    for line in 0..lines.Size()? {
        let words = lines.GetAt(line)?.Words()?;
        let mut recognized_line = Vec::new();
        for index in 0..words.Size()? {
            let word = words.GetAt(index)?;
            let rect = word.BoundingRect()?;
            recognized_line.push(Word {
                bounds: Rect::from_min_size(
                    pos2(rect.X / width as f32, rect.Y / height as f32),
                    vec2(rect.Width / width as f32, rect.Height / height as f32),
                ),
                text: word.Text()?.to_string(),
            });
        }
        if !recognized_line.is_empty() {
            recognized.push(recognized_line);
        }
    }
    Ok(recognized)
}
