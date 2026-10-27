using System.Windows;

namespace ThodView;

public partial class App : Application
{
    protected override void OnStartup(StartupEventArgs e)
    {
        if (e.Args.Contains("--default-viewer") || e.Args.Contains("--uninstall-context") || e.Args.Contains("--register-integrations"))
        {
            StartupUri = null;
            ShutdownMode = ShutdownMode.OnExplicitShutdown;
            try
            {
                if (e.Args.Contains("--uninstall-context")) FileAssociationService.Unregister();
                else { FileAssociationService.Register(); if(e.Args.Contains("--default-viewer")) FileAssociationService.OpenDefaultAppsSettings(); }
                Shutdown(0);
            }
            catch (Exception error) { Console.Error.WriteLine(error.Message); Shutdown(1); }
            return;
        }
        base.OnStartup(e);
        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
        {
            var error = args.ExceptionObject as Exception;
            MessageBox.Show(error?.Message ?? "Неизвестная ошибка.", "ThodView", MessageBoxButton.OK, MessageBoxImage.Error);
        };
    }
}
