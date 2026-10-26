using System.IO;

namespace ThodView;

internal static class ImageFormats
{
    // ffmpeg reads a very wide range of still-image containers. The list is also
    // used for folder navigation, so opening one file immediately enables arrows
    // for every supported image next to it.
    public static readonly string[] Extensions =
    {
        ".png", ".apng", ".jpg", ".jpeg", ".jpe", ".jfif", ".jif", ".jfi",
        ".webp", ".avif", ".heic", ".heif", ".jxl", ".jp2", ".j2k", ".jpf", ".jpx", ".jpm",
        ".bmp", ".dib", ".rle", ".gif", ".tif", ".tiff", ".fax", ".g3",
        ".psd", ".psb", ".tga", ".icb", ".vda", ".vst", ".exr", ".hdr", ".pic",
        ".dds", ".ico", ".cur", ".pnm", ".pgm", ".ppm", ".pbm", ".pam",
        ".pcx", ".qoi", ".sgi", ".rgb", ".rgba", ".bw", ".ras", ".sun",
        ".xbm", ".xpm", ".wbmp", ".wbm", ".hdp", ".wdp", ".jxr",
        ".cr2", ".cr3", ".nef", ".nrw", ".arw", ".srf", ".sr2", ".dng",
        ".raf", ".orf", ".rw2", ".raw", ".pef", ".srw", ".x3f", ".3fr",
        ".erf", ".kdc", ".mef", ".mos", ".mrw"
    };

    public static readonly string[] AssociableExtensions =
    {
        ".png", ".jpg", ".jpeg", ".jpe", ".jfif", ".webp", ".avif", ".heic", ".heif",
        ".jxl", ".jp2", ".bmp", ".dib", ".gif", ".tif", ".tiff", ".psd", ".tga",
        ".exr", ".hdr", ".dds", ".ico", ".apng", ".pnm", ".pgm", ".ppm", ".pbm",
        ".pcx", ".qoi", ".sgi", ".ras", ".xbm", ".xpm", ".wbmp", ".jxr",
        ".cr2", ".cr3", ".nef", ".arw", ".dng", ".raf", ".orf", ".rw2", ".pef", ".srw"
    };

    private static readonly HashSet<string> Set =
        new(Extensions, StringComparer.OrdinalIgnoreCase);

    public static bool IsImage(string path) => Set.Contains(Path.GetExtension(path));

    public static string DialogFilter =>
        "Изображения|" + string.Join(";", Extensions.Select(e => "*" + e)) + "|Все файлы|*.*";
}
