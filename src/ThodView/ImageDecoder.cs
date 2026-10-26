using System.Diagnostics;
using System.IO;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace ThodView;

internal sealed record DecodedImage(byte[] Pixels, int Width, int Height, string? Note);

internal static class ImageDecoder
{
    private const int MaxDimension = 12000;

    public static DecodedImage Decode(string path, string? ffmpeg, string cacheDir)
    {
        string? ffmpegError = null;
        if (!string.IsNullOrWhiteSpace(ffmpeg) && File.Exists(ffmpeg))
        {
            try
            {
                return DecodeWithFfmpeg(path, ffmpeg, cacheDir);
            }
            catch (Exception ex)
            {
                ffmpegError = ex.Message;
            }
        }

        try
        {
            var result = DecodeWithWpf(path);
            if (ffmpegError is not null)
            {
                return result with { Note = "ffmpeg не смог прочитать файл, сработал встроенный декодер Windows. " + ffmpegError };
            }
            if (string.IsNullOrWhiteSpace(ffmpeg))
            {
                return result with { Note = "ffmpeg не найден: открыт встроенным декодером Windows, экзотические форматы могут не работать." };
            }
            return result;
        }
        catch (Exception wpfError)
        {
            var reason = ffmpegError is null ? wpfError.Message : ffmpegError;
            throw new InvalidOperationException("Не удалось открыть изображение. " + reason);
        }
    }

    private static DecodedImage DecodeWithFfmpeg(string path, string ffmpeg, string cacheDir)
    {
        Directory.CreateDirectory(cacheDir);
        var temp = Path.Combine(cacheDir, "decode-" + Guid.NewGuid().ToString("N") + ".png");
        try
        {
            var info = new ProcessStartInfo(ffmpeg)
            {
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            };
            foreach (var argument in new[]
                     {
                         "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
                         "-i", path,
                         "-frames:v", "1",
                         "-vf", $"scale='min(iw,{MaxDimension})':'min(ih,{MaxDimension})':force_original_aspect_ratio=decrease:flags=lanczos",
                         "-pix_fmt", "rgba",
                         "-f", "image2",
                         temp
                     })
            {
                info.ArgumentList.Add(argument);
            }

            using var process = new Process { StartInfo = info };
            if (!process.Start()) throw new IOException("Не удалось запустить ffmpeg.");
            process.StandardInput.Close();
            var stderr = process.StandardError.ReadToEndAsync();
            if (!process.WaitForExit(120_000))
            {
                try { process.Kill(true); } catch { }
                throw new TimeoutException("ffmpeg не успел прочитать изображение за 120 секунд.");
            }
            var errorText = stderr.GetAwaiter().GetResult();
            if (process.ExitCode != 0 || !File.Exists(temp) || new FileInfo(temp).Length == 0)
            {
                var tail = string.Join(" ", errorText.Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).TakeLast(4));
                throw new InvalidOperationException(string.IsNullOrWhiteSpace(tail) ? "ffmpeg завершился с ошибкой." : tail);
            }

            var result = LoadWithWpf(temp);
            var note = Math.Max(result.Width, result.Height) >= MaxDimension
                ? $"Очень большое изображение уменьшено до {MaxDimension} px для просмотра."
                : null;
            return result with { Note = note };
        }
        finally
        {
            try { if (File.Exists(temp)) File.Delete(temp); } catch { }
        }
    }

    private static DecodedImage DecodeWithWpf(string path)
    {
        using var stream = File.OpenRead(path);
        var decoder = BitmapDecoder.Create(stream, BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.OnLoad);
        if (decoder.Frames.Count == 0) throw new InvalidDataException("В файле нет изображения.");
        return Convert(decoder.Frames[0]);
    }

    private static DecodedImage LoadWithWpf(string path)
    {
        using var stream = File.OpenRead(path);
        var decoder = BitmapDecoder.Create(stream, BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.OnLoad);
        if (decoder.Frames.Count == 0) throw new InvalidDataException("В файле нет изображения.");
        return Convert(decoder.Frames[0]);
    }

    private static DecodedImage Convert(BitmapSource frame)
    {
        BitmapSource source = frame;
        var scale = Math.Min(1d, MaxDimension / (double)Math.Max(frame.PixelWidth, frame.PixelHeight));
        if (scale < 0.999)
        {
            source = new TransformedBitmap(frame, new ScaleTransform(scale, scale));
        }
        if (source.Format != PixelFormats.Bgra32)
        {
            source = new FormatConvertedBitmap(source, PixelFormats.Bgra32, null, 0);
        }

        var width = source.PixelWidth;
        var height = source.PixelHeight;
        var stride = width * 4;
        var pixels = new byte[(long)stride * height];
        source.CopyPixels(pixels, stride, 0);
        return new DecodedImage(pixels, width, height, null);
    }
}
