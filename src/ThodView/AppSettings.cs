using System.IO;
using System.Text.Json;

namespace ThodView;

public sealed class AppSettings
{
    public string Theme { get; set; } = "auto";
    public double Width { get; set; } = 1120;
    public double Height { get; set; } = 760;
    public bool Maximized { get; set; }

    public static AppSettings Load(string path)
    {
        try
        {
            if (!File.Exists(path)) return new AppSettings();
            var json = File.ReadAllText(path);
            return JsonSerializer.Deserialize<AppSettings>(json) ?? new AppSettings();
        }
        catch
        {
            return new AppSettings();
        }
    }

    public void Save(string path)
    {
        try
        {
            var directory = Path.GetDirectoryName(path);
            if (!string.IsNullOrEmpty(directory)) Directory.CreateDirectory(directory);
            File.WriteAllText(path, JsonSerializer.Serialize(this, new JsonSerializerOptions { WriteIndented = true }));
        }
        catch
        {
            /* settings are best-effort */
        }
    }
}
