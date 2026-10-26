using System.Windows;

namespace ThodView;

public partial class App : Application
{
    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
        {
            var error = args.ExceptionObject as Exception;
            MessageBox.Show(error?.Message ?? "Неизвестная ошибка.", "ThodView", MessageBoxButton.OK, MessageBoxImage.Error);
        };
    }
}
