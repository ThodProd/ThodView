using System.Windows;

namespace ThodView;

public partial class App : Application
{
    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        if (e.Args.Contains("--default-viewer") || e.Args.Contains("--uninstall-context"))
        {
            StartupUri = null;
            try
            {
                if (e.Args.Contains("--uninstall-context")) FileAssociationService.Unregister();
                else { FileAssociationService.Register(); FileAssociationService.OpenDefaultAppsSettings(); }
                Shutdown(0);
            }
            catch (Exception error) { MessageBox.Show(error.Message, "ThodView"); Shutdown(1); }
            return;
        }
        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
        {
            var error = args.ExceptionObject as Exception;
            MessageBox.Show(error?.Message ?? "Неизвестная ошибка.", "ThodView", MessageBoxButton.OK, MessageBoxImage.Error);
        };
    }
}
