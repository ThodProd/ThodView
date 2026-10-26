using Microsoft.Win32;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;

namespace ThodView;

internal static class FileAssociationService
{
    private const string ProgId = "ThodView.Image";
    private const string AppName = "ThodView";
    private const string Owner = "ThodHUB.ThodView.v1";

    public static void Register()
    {
        var exe = Environment.ProcessPath;
        if (string.IsNullOrEmpty(exe) || !File.Exists(exe))
        {
            throw new InvalidOperationException("Не удалось определить путь к ThodView.exe.");
        }

        var launcher = Environment.GetEnvironmentVariable("THODHUB_LAUNCHER");
        var commandLine = !string.IsNullOrWhiteSpace(launcher) && File.Exists(launcher)
            ? $"\"{launcher}\" --launch thodview \"%1\"" : $"\"{exe}\" \"%1\"";
        using var classes = Registry.CurrentUser.CreateSubKey(@"Software\Classes", true)
                            ?? throw new InvalidOperationException("Нет доступа к HKEY_CURRENT_USER\\Software\\Classes.");

        using (var prog = classes.CreateSubKey(ProgId, true)!)
        {
            prog.SetValue(null, "Изображение ThodView");
            prog.SetValue("ThodHUB.Owner", Owner);
            prog.SetValue("FriendlyTypeName", "Изображение ThodView");
            using (var icon = prog.CreateSubKey("DefaultIcon", true)!)
            {
                icon.SetValue(null, $"\"{exe}\",0");
            }
            using (var command = prog.CreateSubKey(@"shell\open\command", true)!)
            {
                command.SetValue(null, commandLine);
            }
        }

        using (var application = classes.CreateSubKey(@"Applications\ThodView.exe", true)!)
        {
            application.SetValue("FriendlyAppName", AppName);
            application.SetValue("ThodHUB.Owner", Owner);
            using (var command = application.CreateSubKey(@"shell\open\command", true)!)
            {
                command.SetValue(null, commandLine);
            }
            using (var supported = application.CreateSubKey("SupportedTypes", true)!)
            {
                foreach (var extension in ImageFormats.AssociableExtensions)
                {
                    supported.SetValue(extension, "");
                }
            }
        }

        using (var capabilities = Registry.CurrentUser.CreateSubKey(@"Software\ThodView\Capabilities", true)!)
        {
            capabilities.SetValue("ApplicationName", AppName);
            capabilities.SetValue("ThodHUB.Owner", Owner);
            capabilities.SetValue("ApplicationDescription", "Быстрый просмотр изображений через ffmpeg");
            capabilities.SetValue("ApplicationIcon", $"\"{exe}\",0");
            using var associations = capabilities.CreateSubKey("FileAssociations", true)!;
            foreach (var extension in ImageFormats.AssociableExtensions)
            {
                associations.SetValue(extension, ProgId);
            }
        }

        using (var registered = Registry.CurrentUser.CreateSubKey(@"Software\RegisteredApplications", true)!)
        {
            registered.SetValue(AppName, @"Software\ThodView\Capabilities");
        }

        foreach (var extension in ImageFormats.AssociableExtensions)
        {
            using var extensionKey = classes.CreateSubKey(extension, true)!;
            using var openWith = extensionKey.CreateSubKey("OpenWithProgids", true)!;
            openWith.SetValue(ProgId, "", RegistryValueKind.String);
        }

        SHChangeNotify(0x08000000, 0x0000, IntPtr.Zero, IntPtr.Zero);
    }

    public static void OpenDefaultAppsSettings()
    {
        try
        {
            Process.Start(new ProcessStartInfo("ms-settings:defaultapps?registeredAppUser=ThodView")
            {
                UseShellExecute = true
            });
        }
        catch
        {
            Process.Start(new ProcessStartInfo("ms-settings:defaultapps") { UseShellExecute = true });
        }
    }

    public static void Unregister()
    {
        using var owned = Registry.CurrentUser.OpenSubKey(@"Software\Classes\" + ProgId);
        if ((string?)owned?.GetValue("ThodHUB.Owner") != Owner) return;
        foreach (var extension in ImageFormats.AssociableExtensions)
        {
            using var openWith = Registry.CurrentUser.OpenSubKey(@"Software\Classes\" + extension + @"\OpenWithProgids", true);
            openWith?.DeleteValue(ProgId, false);
        }
        foreach (var path in new[] { @"Software\Classes\" + ProgId, @"Software\Classes\Applications\ThodView.exe", @"Software\ThodView\Capabilities" })
        {
            using var key = Registry.CurrentUser.OpenSubKey(path);
            if ((string?)key?.GetValue("ThodHUB.Owner") == Owner) Registry.CurrentUser.DeleteSubKeyTree(path, false);
        }
        using var registered = Registry.CurrentUser.OpenSubKey(@"Software\RegisteredApplications", true);
        if ((string?)registered?.GetValue(AppName) == @"Software\ThodView\Capabilities") registered.DeleteValue(AppName, false);
        SHChangeNotify(0x08000000, 0x0000, IntPtr.Zero, IntPtr.Zero);
    }

    [DllImport("shell32.dll")]
    private static extern void SHChangeNotify(uint eventId, uint flags, IntPtr item1, IntPtr item2);
}
