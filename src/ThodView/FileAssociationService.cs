using Microsoft.Win32;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;

namespace ThodView;

internal static class FileAssociationService
{
    private const string ProgId = "ThodView.Image.Native";
    private const string LegacyProgId = "ThodView.Image";
    private const string AppName = "ThodView";
    private const string Owner = "ThodHUB.ThodView.v1";

    public static void Register()
    {
        var exe = Environment.ProcessPath;
        if (string.IsNullOrEmpty(exe) || !File.Exists(exe))
        {
            throw new InvalidOperationException("Не удалось определить путь к ThodView.exe.");
        }

        // Open With identifies/deduplicates applications by the command executable.
        // A shared hub launcher makes Windows display ThodHUB instead of ThodView.
        // Integration is renewed on every update, so this installed executable stays current.
        var commandLine = $"\"{exe}\" \"%1\"";
        RegisterApplication(exe);
        using var classes = Registry.CurrentUser.CreateSubKey(@"Software\Classes", true)
                            ?? throw new InvalidOperationException("Нет доступа к HKEY_CURRENT_USER\\Software\\Classes.");

        using (var prog = classes.CreateSubKey(ProgId, true)!)
        {
            prog.SetValue(null, "Изображение ThodView");
            prog.SetValue("ThodHUB.Owner", Owner);
            prog.SetValue("FriendlyTypeName", "Изображение ThodView");
            using(var app=prog.CreateSubKey("Application",true)!)
            {
                app.SetValue("ApplicationName",AppName);
                app.SetValue("ApplicationDescription","Просмотр изображений");
                app.SetValue("ApplicationIcon",$"\"{exe}\",0");
            }
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
            using(var icon=application.CreateSubKey("DefaultIcon",true)!)icon.SetValue(null,$"\"{exe}\",0");
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
            openWith.SetValue(ProgId, Array.Empty<byte>(), RegistryValueKind.None);
            openWith.DeleteValue(LegacyProgId, false);
        }

        // Windows caches the old shared-hub identity under the legacy ProgID.
        // Preserve its command for existing user defaults, but advertise only the native identity.
        using(var legacy=classes.OpenSubKey(LegacyProgId,true))
        {
            if(legacy?.GetValue("ThodHUB.Owner") as string==Owner)
            {
                using var command=legacy.CreateSubKey(@"shell\open\command",true)!;command.SetValue(null,commandLine);
            }
        }

        SHChangeNotify(0x08000000, 0x0000, IntPtr.Zero, IntPtr.Zero);
    }

    private static void RegisterApplication(string exe)
    {
        const string path=@"Software\Microsoft\Windows\CurrentVersion\App Paths\ThodView.exe";
        using(var existing=Registry.CurrentUser.OpenSubKey(path))
            if(existing is not null&&existing.GetValue("ThodHUB.Owner") as string!=Owner)throw new IOException("Регистрация ThodView.exe занята другой установкой.");
        using(var key=Registry.CurrentUser.CreateSubKey(path)){key.SetValue(null,exe);key.SetValue("ThodHUB.Owner",Owner);}
        var file=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs),"ThodView.lnk");
        var type=Type.GetTypeFromProgID("WScript.Shell")??throw new IOException("Не удалось создать ярлык в меню «Пуск».");
        object shell=Activator.CreateInstance(type)!;object? shortcut=null;
        try
        {
            dynamic automation=shell;shortcut=automation.CreateShortcut(file);dynamic link=shortcut;
            if(File.Exists(file)&&(string)link.Description!="ThodView · ThodHUB")throw new IOException("В меню «Пуск» уже есть чужой ярлык ThodView.");
            link.TargetPath=exe;link.Arguments="";link.WorkingDirectory=Path.GetDirectoryName(exe)!;link.IconLocation=exe+",0";link.Description="ThodView · ThodHUB";link.Save();
        }
        finally{if(shortcut is not null)Marshal.FinalReleaseComObject(shortcut);Marshal.FinalReleaseComObject(shell);}
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
            openWith?.DeleteValue(ProgId, false);openWith?.DeleteValue(LegacyProgId,false);
        }
        foreach (var path in new[] { @"Software\Classes\" + ProgId, @"Software\Classes\"+LegacyProgId, @"Software\Classes\Applications\ThodView.exe", @"Software\ThodView\Capabilities",@"Software\Microsoft\Windows\CurrentVersion\App Paths\ThodView.exe" })
        {
            using var key = Registry.CurrentUser.OpenSubKey(path);
            if ((string?)key?.GetValue("ThodHUB.Owner") == Owner) Registry.CurrentUser.DeleteSubKeyTree(path, false);
        }
        using var registered = Registry.CurrentUser.OpenSubKey(@"Software\RegisteredApplications", true);
        if ((string?)registered?.GetValue(AppName) == @"Software\ThodView\Capabilities") registered.DeleteValue(AppName, false);
        var file=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs),"ThodView.lnk");
        if(File.Exists(file))
        {
            object shell=Activator.CreateInstance(Type.GetTypeFromProgID("WScript.Shell")!)!;object? shortcut=null;
            try{dynamic automation=shell;shortcut=automation.CreateShortcut(file);dynamic link=shortcut;if((string)link.Description=="ThodView · ThodHUB")File.Delete(file);}
            finally{if(shortcut is not null)Marshal.FinalReleaseComObject(shortcut);Marshal.FinalReleaseComObject(shell);}
        }
        SHChangeNotify(0x08000000, 0x0000, IntPtr.Zero, IntPtr.Zero);
    }

    [DllImport("shell32.dll")]
    private static extern void SHChangeNotify(uint eventId, uint flags, IntPtr item1, IntPtr item2);
}
