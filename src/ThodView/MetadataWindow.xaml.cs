using System.Collections.ObjectModel;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;

namespace ThodView;

public partial class MetadataWindow : Window
{
    private readonly string _path;
    private readonly ObservableCollection<MetaEntry> _rows = new();
    private List<MetaEntry> _original = new();
    private bool _dirty;
    private bool _busy;

    public MetadataWindow(string path)
    {
        InitializeComponent();
        _path = path;
        FileLabel.Text = Path.GetFileName(path);
        MetaGrid.ItemsSource = _rows;
        Loaded += async (_, _) => await ReloadAsync();
    }

    private async Task ReloadAsync()
    {
        if (_busy) return;
        _busy = true;
        StatusText.Text = "Читаю метаданные…";
        try
        {
            var entries = await MetadataService.ReadAsync(_path);
            _original = entries.Select(x => x.Clone()).ToList();
            _rows.Clear();
            foreach (var entry in entries) _rows.Add(entry);
            _dirty = false;
            StatusText.Text = entries.Count == 0
                ? "Метаданных нет. Нажмите «Добавить», чтобы создать."
                : $"Записей: {entries.Count}. Изменяйте значения и нажмите «Применить».";
        }
        catch (Exception ex)
        {
            StatusText.Text = "Ошибка чтения метаданных.";
            MessageBox.Show(this, ex.Message, "Метаданные", MessageBoxButton.OK, MessageBoxImage.Error);
        }
        finally
        {
            _busy = false;
        }
    }

    private void Add_Click(object sender, RoutedEventArgs e)
    {
        var row = new MetaEntry { Key = "XMP:Title", Value = "", Editable = true };
        _rows.Add(row);
        _dirty = true;
        MetaGrid.SelectedItem = row;
        MetaGrid.ScrollIntoView(row);
        MetaGrid.CurrentCell = new DataGridCellInfo(row, MetaGrid.Columns[1]);
        MetaGrid.BeginEdit();
        StatusText.Text = "Новая запись добавлена. Укажите ключ и значение, затем «Применить».";
    }

    private void Delete_Click(object sender, RoutedEventArgs e)
    {
        var selected = MetaGrid.SelectedItems.Cast<MetaEntry>().ToList();
        if (selected.Count == 0) return;
        foreach (var row in selected) _rows.Remove(row);
        _dirty = true;
        StatusText.Text = $"Удалено строк: {selected.Count}. Нажмите «Применить», чтобы изменить файл.";
    }

    private async void Strip_Click(object sender, RoutedEventArgs e)
    {
        var choice = MessageBox.Show(this,
            "Удалить из файла всю метаинформацию? Это действие нельзя отменить.",
            "Стереть всё", MessageBoxButton.YesNo, MessageBoxImage.Warning);
        if (choice != MessageBoxResult.Yes) return;

        try
        {
            StatusText.Text = "Удаляю метаданные…";
            var message = await MetadataService.StripAllAsync(_path);
            await ReloadAsync();
            StatusText.Text = message;
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "Метаданные", MessageBoxButton.OK, MessageBoxImage.Error);
            StatusText.Text = "Не удалось удалить метаданные.";
        }
    }

    private async void Apply_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            StatusText.Text = "Записываю метаданные…";
            var message = await MetadataService.WriteAsync(_path, _rows.ToList(), _original);
            await ReloadAsync();
            StatusText.Text = message;
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "Метаданные", MessageBoxButton.OK, MessageBoxImage.Error);
            StatusText.Text = "Не удалось записать метаданные.";
        }
    }

    private void Copy_Click(object sender, RoutedEventArgs e)
    {
        var selected = MetaGrid.SelectedItems.Cast<MetaEntry>().ToList();
        var list = selected.Count > 0 ? selected : _rows.ToList();
        var text = string.Join(Environment.NewLine,
            list.Select(x => x.Key + " = " + x.Value.Replace("\r", " ").Replace("\n", " ")));
        try
        {
            Clipboard.SetText(text);
            StatusText.Text = $"Скопировано строк: {list.Count}.";
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "Копирование", MessageBoxButton.OK, MessageBoxImage.Error);
        }
    }

    private void Refresh_Click(object sender, RoutedEventArgs e)
    {
        if (_dirty)
        {
            var choice = MessageBox.Show(this, "Отменить несохранённые изменения и перечитать файл?",
                "Метаданные", MessageBoxButton.YesNo, MessageBoxImage.Question);
            if (choice != MessageBoxResult.Yes) return;
        }
        _ = ReloadAsync();
    }

    private void MetaGrid_BeginningEdit(object sender, DataGridBeginningEditEventArgs e)
    {
        if (e.Row.Item is MetaEntry { Editable: false }) e.Cancel = true;
    }

    private void MetaGrid_CellEditEnding(object sender, DataGridCellEditEndingEventArgs e)
    {
        _dirty = true;
    }

    private void Header_MouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ClickCount == 2) return;
        if (e.OriginalSource is DependencyObject source &&
            FindAncestor<Button>(source) is not null) return;
        try { DragMove(); } catch { }
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();

    protected override void OnClosing(System.ComponentModel.CancelEventArgs e)
    {
        if (_dirty)
        {
            var choice = MessageBox.Show(this, "Изменения метаданных не применены. Закрыть окно?",
                "Метаданные", MessageBoxButton.YesNo, MessageBoxImage.Question);
            if (choice != MessageBoxResult.Yes) e.Cancel = true;
        }
        base.OnClosing(e);
    }

    private static T? FindAncestor<T>(DependencyObject? current) where T : DependencyObject
    {
        while (current is not null)
        {
            if (current is T match) return match;
            try { current = System.Windows.Media.VisualTreeHelper.GetParent(current); }
            catch { return null; }
        }
        return null;
    }
}
