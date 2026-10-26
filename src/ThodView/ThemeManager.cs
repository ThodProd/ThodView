using Microsoft.Win32;
using System.Windows;
using System.Windows.Media;

namespace ThodView;

public enum AppTheme
{
    Auto,
    Light,
    Dark
}

public static class ThemeManager
{
    public static bool IsSystemLight()
    {
        try
        {
            using var key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
            return (key?.GetValue("AppsUseLightTheme") is int value) && value != 0;
        }
        catch
        {
            return false;
        }
    }

    public static void Apply(AppTheme mode)
    {
        var light = mode == AppTheme.Light || (mode == AppTheme.Auto && IsSystemLight());
        var resources = Application.Current.Resources;
        if (light)
        {
            resources["AppBg"] = Brush(0xF3, 0xF5, 0xF9);
            resources["PanelBg"] = Brush(0xFF, 0xFF, 0xFF);
            resources["CanvasBg"] = Brush(0xE8, 0xEC, 0xF3);
            resources["TextPrimary"] = Brush(0x13, 0x18, 0x20);
            resources["TextMuted"] = Brush(0x5F, 0x6A, 0x7C);
            resources["LineBrush"] = Brush(0xDC, 0xE2, 0xEC);
            resources["HoverBrush"] = Brush(0xED, 0xF1, 0xF7);
            resources["AccentBrush"] = Brush(0x2F, 0x6F, 0xED);
        }
        else
        {
            resources["AppBg"] = Brush(0x0E, 0x10, 0x15);
            resources["PanelBg"] = Brush(0x15, 0x19, 0x20);
            resources["CanvasBg"] = Brush(0x0B, 0x0D, 0x11);
            resources["TextPrimary"] = Brush(0xED, 0xF1, 0xF7);
            resources["TextMuted"] = Brush(0x8B, 0x94, 0xA6);
            resources["LineBrush"] = Brush(0x24, 0x2A, 0x35);
            resources["HoverBrush"] = Brush(0x1D, 0x23, 0x30);
            resources["AccentBrush"] = Brush(0x5B, 0x8C, 0xFF);
        }
    }

    private static SolidColorBrush Brush(byte r, byte g, byte b)
    {
        var brush = new SolidColorBrush(Color.FromRgb(r, g, b));
        brush.Freeze();
        return brush;
    }
}

