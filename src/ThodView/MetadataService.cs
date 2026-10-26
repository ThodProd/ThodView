using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;

namespace ThodView;

internal sealed class MetaEntry : INotifyPropertyChanged
{
    private string _key = "";
    private string _value = "";

    public string Key
    {
        get => _key;
        set
        {
            if (_key == value) return;
            _key = value;
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Key)));
        }
    }

    public string Value
    {
        get => _value;
        set
        {
            if (_value == value) return;
            _value = value;
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Value)));
        }
    }

    public bool Editable { get; init; } = true;

    public MetaEntry Clone() => new() { Key = Key, Value = Value, Editable = Editable };

    public event PropertyChangedEventHandler? PropertyChanged;
}

internal static class MetadataService
{
    private static readonly HashSet<string> ReadOnlyGroups = new(StringComparer.OrdinalIgnoreCase)
    {
        "File", "ExifTool", "System", "Composite", "ICC_Profile", "JFIF",
        "MakerNotes", "Preview", "MPF", "Vendor", "Track1", "Track2"
    };

    private static readonly HashSet<string> ReadOnlyTags = new(StringComparer.OrdinalIgnoreCase)
    {
        "SourceFile", "ExifToolVersion", "FileName", "Directory", "FileSize",
        "FileModifyDate", "FileAccessDate", "FileInodeChangeDate", "FilePermissions",
        "FileType", "FileTypeExtension", "MIMEType", "ImageWidth", "ImageHeight",
        "ImageSize", "Megapixels", "EncodingProcess", "BitsPerSample",
        "ColorComponents", "YCbCrSubSampling", "ThumbnailImage", "PreviewImage"
    };

    private static readonly HashSet<string> PngTextTags = new(StringComparer.OrdinalIgnoreCase)
    {
        "Comment", "Description", "Title", "Author", "Copyright", "CreationTime",
        "Software", "Disclaimer", "Warning", "Source", "XML"
    };

    public static async Task<List<MetaEntry>> ReadAsync(string path)
    {
        var exe = FindExifTool();
        var result = await RunAsync(exe, new[]
        {
            "-j", "-G1", "-a", "-s", "-charset", "filename=UTF8", "--", path
        });

        if (result.ExitCode != 0)
        {
            throw new InvalidOperationException("ExifTool не смог прочитать метаданные.\n" +
                                                Tail(result.Error, 6));
        }
        if (string.IsNullOrWhiteSpace(result.Output)) return new List<MetaEntry>();

        using var document = JsonDocument.Parse(result.Output);
        if (document.RootElement.ValueKind != JsonValueKind.Array || document.RootElement.GetArrayLength() == 0)
        {
            return new List<MetaEntry>();
        }

        var entries = new List<MetaEntry>();
        foreach (var property in document.RootElement[0].EnumerateObject())
        {
            var key = property.Name;
            var colon = key.IndexOf(':');
            var group = colon > 0 ? key[..colon] : "";
            var tag = colon > 0 ? key[(colon + 1)..] : key;
            entries.Add(new MetaEntry
            {
                Key = key,
                Value = JsonText(property.Value),
                Editable = IsEditable(group, tag)
            });
        }

        return entries
            .OrderBy(x => x.Key.StartsWith("File:", StringComparison.OrdinalIgnoreCase) ? 1 : 0)
            .ThenBy(x => x.Key, StringComparer.CurrentCultureIgnoreCase)
            .ToList();
    }

    public static async Task<string> WriteAsync(string path, IReadOnlyList<MetaEntry> current,
        IReadOnlyList<MetaEntry> original)
    {
        var exe = FindExifTool();
        var currentMap = current
            .Where(x => x.Editable && !string.IsNullOrWhiteSpace(x.Key))
            .GroupBy(x => x.Key, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(x => x.Key, x => x.First().Value, StringComparer.OrdinalIgnoreCase);
        var originalMap = original
            .Where(x => x.Editable && !string.IsNullOrWhiteSpace(x.Key))
            .GroupBy(x => x.Key, StringComparer.OrdinalIgnoreCase)
            .ToDictionary(x => x.Key, x => x.First().Value, StringComparer.OrdinalIgnoreCase);

        var lines = new List<string> { "-overwrite_original" };
        foreach (var pair in currentMap)
        {
            if (!originalMap.TryGetValue(pair.Key, out var oldValue) ||
                !string.Equals(oldValue, pair.Value, StringComparison.Ordinal))
            {
                lines.Add("-" + pair.Key + "=" + CleanValue(pair.Value));
            }
        }
        foreach (var key in originalMap.Keys)
        {
            if (!currentMap.ContainsKey(key)) lines.Add("-" + key + "=");
        }

        if (lines.Count == 1) return "Нет изменений для записи.";
        lines.Add(path);

        // Unicode values are safer in a UTF-8 argument file: the Windows
        // ExifTool launcher does not always preserve non-ASCII command-line args.
        var argumentFile = Path.Combine(Path.GetTempPath(), "thodview-meta-" + Guid.NewGuid().ToString("N") + ".txt");
        try
        {
            await File.WriteAllLinesAsync(argumentFile, lines, new UTF8Encoding(false));
            var result = await RunAsync(exe, new[]
            {
                "-charset", "filename=UTF8", "-@", argumentFile
            });
            if (result.ExitCode != 0)
            {
                throw new InvalidOperationException("ExifTool не смог записать метаданные.\n" +
                                                    Tail(result.Error, 6));
            }
            return string.IsNullOrWhiteSpace(result.Error)
                ? "Метаданные сохранены."
                : "Метаданные сохранены. Предупреждения:\n" + Tail(result.Error, 4);
        }
        finally
        {
            try { File.Delete(argumentFile); } catch { }
        }
    }

    public static async Task<string> StripAllAsync(string path)
    {
        var exe = FindExifTool();
        var result = await RunAsync(exe, new[]
        {
            "-all=", "-overwrite_original", "-charset", "filename=UTF8", "--", path
        });
        if (result.ExitCode != 0)
        {
            throw new InvalidOperationException("ExifTool не смог удалить метаданные.\n" +
                                                Tail(result.Error, 6));
        }
        return "Все метаданные удалены.";
    }

    private static string FindExifTool()
    {
        var explicitPath = Environment.GetEnvironmentVariable("THODVIEW_EXIFTOOL");
        if (!string.IsNullOrWhiteSpace(explicitPath) && File.Exists(explicitPath)) return explicitPath;

        var local = Path.Combine(AppContext.BaseDirectory, "runtime", "exiftool", "ExifTool.exe");
        if (File.Exists(local)) return local;

        var process = Environment.ProcessPath;
        if (!string.IsNullOrEmpty(process))
        {
            var next = Path.Combine(Path.GetDirectoryName(process) ?? "", "runtime", "exiftool", "ExifTool.exe");
            if (File.Exists(next)) return next;
        }

        var path = Environment.GetEnvironmentVariable("PATH");
        if (!string.IsNullOrWhiteSpace(path))
        {
            foreach (var directory in path.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
            {
                try
                {
                    foreach (var name in new[] { "ExifTool.exe", "exiftool.exe" })
                    {
                        var candidate = Path.Combine(directory.Trim(), name);
                        if (File.Exists(candidate)) return candidate;
                    }
                }
                catch
                {
                    /* malformed PATH entries are ignored */
                }
            }
        }

        throw new FileNotFoundException(
            "ExifTool не найден. Установите пакет ThodView с папкой runtime\\exiftool или задайте THODVIEW_EXIFTOOL.");
    }

    private static bool IsEditable(string group, string tag)
    {
        if (string.IsNullOrWhiteSpace(tag)) return false;
        if (ReadOnlyGroups.Contains(group)) return false;
        if (ReadOnlyTags.Contains(tag)) return false;
        if (group.Equals("PNG", StringComparison.OrdinalIgnoreCase) && !PngTextTags.Contains(tag)) return false;
        return true;
    }

    private static string JsonText(JsonElement element)
    {
        return element.ValueKind switch
        {
            JsonValueKind.String => element.GetString() ?? "",
            JsonValueKind.Number => element.GetRawText(),
            JsonValueKind.True => "true",
            JsonValueKind.False => "false",
            JsonValueKind.Array => string.Join("; ", element.EnumerateArray().Select(JsonText)),
            JsonValueKind.Null => "",
            _ => element.GetRawText()
        };
    }

    private static async Task<(int ExitCode, string Output, string Error)> RunAsync(
        string executable, IEnumerable<string> arguments)
    {
        var start = new ProcessStartInfo(executable)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8
        };
        foreach (var argument in arguments) start.ArgumentList.Add(argument);

        using var process = new Process { StartInfo = start };
        if (!process.Start()) throw new IOException("Не удалось запустить ExifTool.");

        var outputTask = process.StandardOutput.ReadToEndAsync();
        var errorTask = process.StandardError.ReadToEndAsync();
        if (!process.WaitForExit(120_000))
        {
            try { process.Kill(true); } catch { }
            throw new TimeoutException("ExifTool не ответил за 120 секунд.");
        }
        return (process.ExitCode, await outputTask, await errorTask);
    }

    private static string CleanValue(string value) =>
        value.Replace("\r\n", " ").Replace('\r', ' ').Replace('\n', ' ');

    private static string Tail(string text, int lines)
    {
        if (string.IsNullOrWhiteSpace(text)) return "Без подробностей.";
        return string.Join(Environment.NewLine, text
            .Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .TakeLast(lines));
    }
}
