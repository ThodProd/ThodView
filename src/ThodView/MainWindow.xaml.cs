using Microsoft.Win32;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace ThodView;

public partial class MainWindow : Window
{
    private const double MinScale = 0.01;
    private const double MaxScale = 64;
    private const int WM_GETMINMAXINFO = 0x0024;

    private readonly string _dataDir;
    private readonly string _settingsPath;
    private readonly AppSettings _settings;
    private readonly string? _ffmpeg;
    private readonly DispatcherTimer _fadeTimer;

    private byte[]? _pixels;
    private int _width;
    private int _height;
    private double _scale = 1;
    private double _offsetX;
    private double _offsetY;
    private bool _fitMode = true;
    private bool _panning;
    private Point _panStart;
    private double _panOffsetX;
    private double _panOffsetY;
    private bool _cropMode;
    private bool _cropDragging;
    private Point _cropStart;
    private Point _cropEnd;
    private bool _loading;
    private bool _dirty;
    private bool _toolbarOpen;
    private string? _filePath;
    private readonly List<string> _folderFiles = new();
    private int _folderIndex = -1;
    private AppTheme _theme;
    private HwndSource? _source;
    private IntPtr _hwnd;

    public MainWindow()
    {
        InitializeComponent();

        _dataDir = Environment.GetEnvironmentVariable("THODHUB_DATA_DIR")
                   ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ThodView");
        Directory.CreateDirectory(_dataDir);
        _settingsPath = Path.Combine(_dataDir, "settings.json");
        _settings = AppSettings.Load(_settingsPath);
        _theme = ParseTheme(_settings.Theme);
        _ffmpeg = FindFfmpeg();

        var work = SystemParameters.WorkArea;
        Width = Math.Max(MinWidth, Math.Min(_settings.Width, work.Width));
        Height = Math.Max(MinHeight, Math.Min(_settings.Height, work.Height));
        if (_settings.Maximized) WindowState = WindowState.Maximized;

        _fadeTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(2.4) };
        _fadeTimer.Tick += (_, _) =>
        {
            if (GearPanel.Visibility == Visibility.Visible || GearButton.IsMouseOver) return;
            var animation = new DoubleAnimation(0.18, TimeSpan.FromMilliseconds(650))
            {
                EasingFunction = new QuadraticEase { EasingMode = EasingMode.EaseOut }
            };
            GearButton.BeginAnimation(OpacityProperty, animation);
        };

        PreviewImage.RenderTransform = new MatrixTransform();
        ThemeManager.Apply(_theme);
        ApplyThemeLabel();
        CleanCache();

        Loaded += OnLoaded;
        PreviewMouseDown += (_, e) =>
        {
            if (GearPanel.Visibility != Visibility.Visible) return;
            var source = e.OriginalSource as DependencyObject;
            if (FindAncestor<ButtonBase>(source) is not null) return;
            if (IsInside(source, GearPanel) || IsInside(source, GearButton)) return;
            GearPanel.Visibility = Visibility.Collapsed;
        };
    }

    protected override void OnSourceInitialized(EventArgs e)
    {
        base.OnSourceInitialized(e);
        _hwnd = new WindowInteropHelper(this).Handle;
        _source = HwndSource.FromHwnd(_hwnd);
        _source?.AddHook(WndProc);
    }

    private async void OnLoaded(object sender, RoutedEventArgs e)
    {
        ShowGear();
        if (_theme == AppTheme.Auto) ApplyThemeLabel();

        var arguments = Environment.GetCommandLineArgs().Skip(1).ToArray();
        var file = arguments.FirstOrDefault(x => !x.StartsWith('-') && File.Exists(x));
        if (file is not null) await OpenFileAsync(file);
    }

    /* ------------------------------------------------------------------ */
    /* Opening images                                                      */
    /* ------------------------------------------------------------------ */

    private static string? FindFfmpeg()
    {
        var explicitPath = Environment.GetEnvironmentVariable("THODVIEW_FFMPEG");
        if (!string.IsNullOrWhiteSpace(explicitPath) && File.Exists(explicitPath)) return explicitPath;

        var local = Path.Combine(AppContext.BaseDirectory, "runtime", "ffmpeg", "ffmpeg.exe");
        if (File.Exists(local)) return local;

        var path = Environment.GetEnvironmentVariable("PATH");
        if (!string.IsNullOrWhiteSpace(path))
        {
            foreach (var directory in path.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries))
            {
                try
                {
                    var candidate = Path.Combine(directory.Trim(), "ffmpeg.exe");
                    if (File.Exists(candidate)) return candidate;
                }
                catch
                {
                    /* malformed PATH entries are ignored */
                }
            }
        }
        return null;
    }

    private void Open_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new OpenFileDialog
        {
            Title = "Открыть изображение",
            CheckFileExists = true,
            Filter = ImageFormats.DialogFilter
        };
        if (dialog.ShowDialog(this) == true) _ = OpenFileAsync(dialog.FileName);
    }

    private async Task<bool> OpenFileAsync(string path, bool preserveFolder = false)
    {
        if (_loading) return false;
        if (!File.Exists(path))
        {
            MessageBox.Show(this, "Файл не найден:\n" + path, "ThodView", MessageBoxButton.OK, MessageBoxImage.Warning);
            return false;
        }

        _loading = true;
        ShowGear();
        LoadingText.Text = "Открываю " + Path.GetFileName(path) + "…";
        LoadingOverlay.Visibility = Visibility.Visible;
        try
        {
            var dataDir = _dataDir;
            var ffmpeg = _ffmpeg;
            var decoded = await Task.Run(() => ImageDecoder.Decode(path, ffmpeg, Path.Combine(dataDir, "cache")));

            _pixels = decoded.Pixels;
            _width = decoded.Width;
            _height = decoded.Height;
            _filePath = Path.GetFullPath(path);
            _dirty = false;
            PreviewImage.Source = CreateBitmap();
            PreviewImage.Visibility = Visibility.Visible;
            EmptyState.Visibility = Visibility.Collapsed;
            if (!preserveFolder || FindFolderIndex(_filePath) < 0)
            {
                RefreshFolderList(_filePath);
            }
            else
            {
                _folderIndex = FindFolderIndex(_filePath);
            }

            FitToWindow();
            UpdateTitle();
            UpdateStatus();
            UpdateFolderCounter();
            UpdateFolderNavVisibility();
            StatusLeft.ToolTip = _filePath;
            if (decoded.Note is not null) StatusCenter.Text += " · уменьшено";
            Viewport.Focus();
            return true;
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "Не удалось открыть изображение", MessageBoxButton.OK, MessageBoxImage.Error);
            if (_pixels is null) EmptyState.Visibility = Visibility.Visible;
            return false;
        }
        finally
        {
            _loading = false;
            LoadingOverlay.Visibility = Visibility.Collapsed;
        }
    }

    private void RefreshFolderList(string path)
    {
        _folderFiles.Clear();
        _folderIndex = -1;
        var full = Path.GetFullPath(path);
        var folder = Path.GetDirectoryName(full);
        if (!string.IsNullOrEmpty(folder))
        {
            try
            {
                _folderFiles.AddRange(Directory.EnumerateFiles(folder)
                    .Where(ImageFormats.IsImage)
                    .OrderBy(x => Path.GetFileName(x), StringComparer.CurrentCultureIgnoreCase));
            }
            catch
            {
                /* the current image still opens even if the folder is unreadable */
            }
        }

        if (!_folderFiles.Any(x => string.Equals(x, full, StringComparison.OrdinalIgnoreCase)))
        {
            _folderFiles.Add(full);
            _folderFiles.Sort((a, b) => string.Compare(Path.GetFileName(a), Path.GetFileName(b),
                StringComparison.CurrentCultureIgnoreCase));
        }
        _folderIndex = FindFolderIndex(full);
    }

    private int FindFolderIndex(string? path)
    {
        if (path is null) return -1;
        for (var i = 0; i < _folderFiles.Count; i++)
        {
            if (string.Equals(_folderFiles[i], path, StringComparison.OrdinalIgnoreCase)) return i;
        }
        return -1;
    }

    private async Task SwitchImageAsync(int delta)
    {
        if (_loading || _folderFiles.Count < 2) return;
        if (_dirty)
        {
            var choice = MessageBox.Show(this, "Есть несохранённые изменения. Сохранить перед переходом?",
                "ThodView", MessageBoxButton.YesNoCancel, MessageBoxImage.Question);
            if (choice == MessageBoxResult.Cancel) return;
            if (choice == MessageBoxResult.Yes && !SaveAs()) return;
        }

        var index = FindFolderIndex(_filePath);
        if (index < 0) return;
        var next = (index + delta + _folderFiles.Count) % _folderFiles.Count;
        if (next == index) return;
        var target = _folderFiles[next];
        _folderIndex = next;
        UpdateFolderCounter();
        if (!await OpenFileAsync(target, preserveFolder: true))
        {
            _folderIndex = index;
            UpdateFolderCounter();
        }
        UpdateFolderNavVisibility(Mouse.GetPosition(Viewport));
    }

    private void Previous_Click(object sender, RoutedEventArgs e) => _ = SwitchImageAsync(-1);

    private void Next_Click(object sender, RoutedEventArgs e) => _ = SwitchImageAsync(1);

    private void FolderNav_MouseEnter(object sender, MouseEventArgs e) => UpdateFolderNavVisibility();

    private void FolderNav_MouseLeave(object sender, MouseEventArgs e)
    {
        UpdateFolderNavVisibility(Mouse.GetPosition(Viewport));
    }

    private void UpdateFolderCounter()
    {
        FolderCounter.Text = _folderFiles.Count == 0
            ? ""
            : $"{Math.Max(1, _folderIndex + 1)} / {_folderFiles.Count}";
    }

    private void UpdateFolderNavVisibility(Point? point = null)
    {
        var nearBottom = point.HasValue && point.Value.Y >= Viewport.ActualHeight - 96;
        var show = _pixels is not null && !_cropMode && _folderFiles.Count > 1 &&
                   (nearBottom || FolderNav.IsMouseOver);
        FolderNav.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
    }

    private BitmapSource CreateBitmap()
    {
        if (_pixels is null) throw new InvalidOperationException("Нет изображения.");
        var bitmap = BitmapSource.Create(_width, _height, 96, 96, PixelFormats.Bgra32, null, _pixels, _width * 4);
        bitmap.Freeze();
        return bitmap;
    }

    /* ------------------------------------------------------------------ */
    /* View transform                                                      */
    /* ------------------------------------------------------------------ */

    private void Viewport_SizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (_pixels is null || !IsLoaded) return;
        if (_fitMode) FitToWindow();
        else { ClampOffsets(); UpdateTransform(); }
    }

    private void FitToWindow()
    {
        if (_pixels is null) return;
        var vw = Viewport.ActualWidth;
        var vh = Viewport.ActualHeight;
        if (vw < 8 || vh < 8) return;

        var scale = Math.Min((vw - 48) / _width, (vh - 48) / _height);
        _scale = Math.Clamp(scale, MinScale, 16);
        _offsetX = (vw - _width * _scale) / 2;
        _offsetY = (vh - _height * _scale) / 2;
        _fitMode = true;
        UpdateTransform();
    }

    private void ActualSize()
    {
        if (_pixels is null) return;
        var vw = Viewport.ActualWidth;
        var vh = Viewport.ActualHeight;
        _scale = 1;
        _offsetX = vw / 2 - _width / 2d;
        _offsetY = vh / 2 - _height / 2d;
        _fitMode = false;
        ClampOffsets();
        UpdateTransform();
    }

    private void ZoomCenter(double factor)
    {
        if (_pixels is null) return;
        ZoomAt(new Point(Viewport.ActualWidth / 2, Viewport.ActualHeight / 2), factor);
    }

    private void ZoomAt(Point point, double factor)
    {
        if (_pixels is null) return;
        var next = Math.Clamp(_scale * factor, MinScale, MaxScale);
        if (Math.Abs(next - _scale) < 0.000001) return;
        var imageX = (point.X - _offsetX) / _scale;
        var imageY = (point.Y - _offsetY) / _scale;
        _scale = next;
        _offsetX = point.X - imageX * next;
        _offsetY = point.Y - imageY * next;
        _fitMode = false;
        ClampOffsets();
        UpdateTransform();
    }

    private void ClampOffsets()
    {
        var vw = Viewport.ActualWidth;
        var vh = Viewport.ActualHeight;
        if (vw < 1 || vh < 1) return;
        var dw = _width * _scale;
        var dh = _height * _scale;
        _offsetX = dw <= vw ? (vw - dw) / 2 : Math.Clamp(_offsetX, vw - dw, 0);
        _offsetY = dh <= vh ? (vh - dh) / 2 : Math.Clamp(_offsetY, vh - dh, 0);
    }

    private void UpdateTransform()
    {
        PreviewImage.Width = _width;
        PreviewImage.Height = _height;
        var matrix = Matrix.Identity;
        matrix.Scale(_scale, _scale);
        matrix.Translate(_offsetX, _offsetY);
        PreviewImage.RenderTransform = new MatrixTransform(matrix);
        UpdateZoomText();
        HideToolbarIfFitted();
    }

    private Rect ImageRect() => new(_offsetX, _offsetY, _width * _scale, _height * _scale);

    /* ------------------------------------------------------------------ */
    /* Mouse input                                                         */
    /* ------------------------------------------------------------------ */

    private void Viewport_MouseWheel(object sender, MouseWheelEventArgs e)
    {
        if (_pixels is null || _cropMode) return;
        ZoomAt(e.GetPosition(Viewport), Math.Pow(1.2, e.Delta / 120.0));
        e.Handled = true;
    }

    private void Viewport_MouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        if (_pixels is null || FindAncestor<ButtonBase>(e.OriginalSource as DependencyObject) is not null) return;
        Viewport.Focus();
        var point = e.GetPosition(Viewport);
        if (_cropMode)
        {
            _cropDragging = true;
            _cropStart = point;
            _cropEnd = point;
            Viewport.CaptureMouse();
            UpdateCropVisual();
            e.Handled = true;
            return;
        }

        _panning = true;
        _panStart = point;
        _panOffsetX = _offsetX;
        _panOffsetY = _offsetY;
        Viewport.CaptureMouse();
        Viewport.Cursor = Cursors.SizeAll;
        e.Handled = true;
    }

    private void Viewport_MouseMove(object sender, MouseEventArgs e)
    {
        if (_pixels is null) return;
        var point = e.GetPosition(Viewport);
        UpdateFolderNavVisibility(point);
        if (_cropMode && _cropDragging)
        {
            _cropEnd = point;
            UpdateCropVisual();
            return;
        }
        if (!_panning) return;
        _offsetX = _panOffsetX + (point.X - _panStart.X);
        _offsetY = _panOffsetY + (point.Y - _panStart.Y);
        ClampOffsets();
        UpdateTransform();
    }

    private void Viewport_MouseLeftButtonUp(object sender, MouseButtonEventArgs e)
    {
        if (_panning)
        {
            _panning = false;
            Viewport.ReleaseMouseCapture();
            Viewport.Cursor = _pixels is null ? Cursors.Arrow : Cursors.Hand;
        }
        if (_cropDragging)
        {
            _cropDragging = false;
            Viewport.ReleaseMouseCapture();
        }
    }

    private void Viewport_MouseLeave(object sender, MouseEventArgs e)
    {
        if (!_panning && !_cropDragging) Viewport.Cursor = _pixels is null ? Cursors.Arrow : Cursors.Hand;
        if (!FolderNav.IsMouseOver) FolderNav.Visibility = Visibility.Collapsed;
    }

    /* ------------------------------------------------------------------ */
    /* Crop                                                                */
    /* ------------------------------------------------------------------ */

    private void Crop_Click(object sender, RoutedEventArgs e) => ToggleCrop();

    private void ToggleCrop()
    {
        if (_pixels is null) return;
        if (_cropMode) { ExitCrop(); return; }

        GearPanel.Visibility = Visibility.Collapsed;
        HideToolbar();
        _cropMode = true;
        _cropDragging = false;
        UpdateFolderNavVisibility();
        CropCanvas.Visibility = Visibility.Visible;
        CropBar.Visibility = Visibility.Visible;
        StatusLeft.Text = "Обрезка: выделите область мышью";
        Viewport.Cursor = Cursors.Cross;

        var image = ImageRect();
        image.Intersect(new Rect(0, 0, Viewport.ActualWidth, Viewport.ActualHeight));
        _cropStart = image.TopLeft;
        _cropEnd = image.BottomRight;
        UpdateCropVisual();
    }

    private void ExitCrop()
    {
        _cropMode = false;
        _cropDragging = false;
        CropCanvas.Visibility = Visibility.Collapsed;
        CropBar.Visibility = Visibility.Collapsed;
        Viewport.Cursor = _pixels is null ? Cursors.Arrow : Cursors.Hand;
        UpdateFolderNavVisibility();
        UpdateStatus();
    }

    private Point ClampToImage(Point point)
    {
        var image = ImageRect();
        return new Point(
            Math.Clamp(point.X, image.Left, image.Right),
            Math.Clamp(point.Y, image.Top, image.Bottom));
    }

    private Rect SelectionRect()
    {
        var a = ClampToImage(_cropStart);
        var b = ClampToImage(_cropEnd);
        var rect = new Rect(a, b);
        rect.Intersect(ImageRect());
        return rect;
    }

    private void UpdateCropVisual()
    {
        var rect = SelectionRect();
        if (rect.Width < 0) rect = new Rect(rect.X, rect.Y, 0, rect.Height);
        if (rect.Height < 0) rect = new Rect(rect.X, rect.Y, rect.Width, 0);
        var vw = Viewport.ActualWidth;
        var vh = Viewport.ActualHeight;

        SetRect(CropTop, 0, 0, vw, Math.Max(0, rect.Top));
        SetRect(CropBottom, 0, rect.Bottom, vw, Math.Max(0, vh - rect.Bottom));
        SetRect(CropLeft, 0, rect.Top, Math.Max(0, rect.Left), Math.Max(0, rect.Height));
        SetRect(CropRight, rect.Right, rect.Top, Math.Max(0, vw - rect.Right), Math.Max(0, rect.Height));
        SetRect(CropSelection, rect.Left, rect.Top, Math.Max(0, rect.Width), Math.Max(0, rect.Height));
    }

    private static void SetRect(FrameworkElement element, double x, double y, double width, double height)
    {
        Canvas.SetLeft(element, x);
        Canvas.SetTop(element, y);
        element.Width = Math.Max(0, width);
        element.Height = Math.Max(0, height);
    }

    private void CropApply_Click(object sender, RoutedEventArgs e)
    {
        if (!_cropMode || _pixels is null) return;
        var rect = SelectionRect();
        var x = (int)Math.Round((rect.Left - _offsetX) / _scale);
        var y = (int)Math.Round((rect.Top - _offsetY) / _scale);
        var w = (int)Math.Round(rect.Width / _scale);
        var h = (int)Math.Round(rect.Height / _scale);
        x = Math.Clamp(x, 0, _width - 1);
        y = Math.Clamp(y, 0, _height - 1);
        w = Math.Clamp(w, 1, _width - x);
        h = Math.Clamp(h, 1, _height - y);
        if (w < 2 || h < 2)
        {
            MessageBox.Show(this, "Слишком маленькая область. Выделите хотя бы пару пикселей.", "Обрезка",
                MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }

        var cropped = new byte[(long)w * h * 4];
        for (var row = 0; row < h; row++)
        {
            Buffer.BlockCopy(_pixels, ((y + row) * _width + x) * 4, cropped, row * w * 4, w * 4);
        }

        _pixels = cropped;
        _width = w;
        _height = h;
        _dirty = true;
        ExitCrop();
        PreviewImage.Source = CreateBitmap();
        FitToWindow();
        UpdateTitle();
        UpdateStatus();
    }

    private void CropCancel_Click(object sender, RoutedEventArgs e) => ExitCrop();

    private void ActualSize_Click(object sender, RoutedEventArgs e) => ActualSize();
    private void Fit_Click(object sender, RoutedEventArgs e) => FitToWindow();

    /* ------------------------------------------------------------------ */
    /* Pixel edits                                                         */
    /* ------------------------------------------------------------------ */

    private void RotateLeft_Click(object sender, RoutedEventArgs e) => Rotate(false);
    private void RotateRight_Click(object sender, RoutedEventArgs e) => Rotate(true);
    private void FlipHorizontal_Click(object sender, RoutedEventArgs e) => Flip(true);
    private void FlipVertical_Click(object sender, RoutedEventArgs e) => Flip(false);

    private void Rotate(bool clockwise)
    {
        if (_pixels is null || _loading) return;
        if (_cropMode) ExitCrop();
        var source = _pixels;
        var width = _width;
        var height = _height;
        var result = new byte[source.Length];
        var newWidth = height;
        for (var y = 0; y < height; y++)
        {
            for (var x = 0; x < width; x++)
            {
                var nx = clockwise ? height - 1 - y : y;
                var ny = clockwise ? x : width - 1 - x;
                Buffer.BlockCopy(source, (y * width + x) * 4, result, (ny * newWidth + nx) * 4, 4);
            }
        }
        _pixels = result;
        _width = height;
        _height = width;
        AfterEdit();
    }

    private void Flip(bool horizontal)
    {
        if (_pixels is null || _loading) return;
        if (_cropMode) ExitCrop();
        var source = _pixels;
        var width = _width;
        var height = _height;
        var result = new byte[source.Length];
        for (var y = 0; y < height; y++)
        {
            for (var x = 0; x < width; x++)
            {
                var nx = horizontal ? width - 1 - x : x;
                var ny = horizontal ? y : height - 1 - y;
                Buffer.BlockCopy(source, (y * width + x) * 4, result, (ny * width + nx) * 4, 4);
            }
        }
        _pixels = result;
        AfterEdit();
    }

    private void AfterEdit()
    {
        _dirty = true;
        PreviewImage.Source = CreateBitmap();
        FitToWindow();
        UpdateTitle();
        UpdateStatus();
    }

    private void EditToggle_Click(object sender, RoutedEventArgs e)
    {
        if (_pixels is null) return;
        if (_toolbarOpen) HideToolbar();
        else ShowToolbar();
    }

    private void ShowToolbar()
    {
        if (_pixels is null) return;
        _toolbarOpen = true;
        EditToolbar.Visibility = Visibility.Visible;
        ToolbarTransform.BeginAnimation(TranslateTransform.XProperty, null);
        EditToolbar.BeginAnimation(OpacityProperty, null);
        ToolbarTransform.X = -18;
        EditToolbar.Opacity = 0;
        var slide = new DoubleAnimation(0, TimeSpan.FromMilliseconds(180))
        {
            EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut }
        };
        var fade = new DoubleAnimation(1, TimeSpan.FromMilliseconds(160));
        ToolbarTransform.BeginAnimation(TranslateTransform.XProperty, slide);
        EditToolbar.BeginAnimation(OpacityProperty, fade);
        EditToggleButton.ToolTip = "Скрыть инструменты";
    }

    private void HideToolbar()
    {
        if (!_toolbarOpen)
        {
            EditToolbar.Visibility = Visibility.Collapsed;
            return;
        }
        _toolbarOpen = false;
        var slide = new DoubleAnimation(-18, TimeSpan.FromMilliseconds(150))
        {
            EasingFunction = new CubicEase { EasingMode = EasingMode.EaseIn }
        };
        var fade = new DoubleAnimation(0, TimeSpan.FromMilliseconds(130));
        fade.Completed += (_, _) =>
        {
            if (_toolbarOpen) return;
            EditToolbar.Visibility = Visibility.Collapsed;
            ToolbarTransform.X = -18;
            EditToolbar.Opacity = 1;
        };
        ToolbarTransform.BeginAnimation(TranslateTransform.XProperty, slide);
        EditToolbar.BeginAnimation(OpacityProperty, fade);
        EditToggleButton.ToolTip = "Показать инструменты (поворот, отражение, обрезка)";
    }

    private bool IsImageFullyVisible()
    {
        if (_pixels is null || Viewport.ActualWidth < 1 || Viewport.ActualHeight < 1) return false;
        var rect = ImageRect();
        const double tolerance = 1.5;
        return rect.Left >= -tolerance && rect.Top >= -tolerance &&
               rect.Right <= Viewport.ActualWidth + tolerance &&
               rect.Bottom <= Viewport.ActualHeight + tolerance;
    }

    private void HideToolbarIfFitted()
    {
        if (_toolbarOpen && IsImageFullyVisible()) HideToolbar();
    }

    /* ------------------------------------------------------------------ */
    /* Saving                                                              */
    /* ------------------------------------------------------------------ */

    private void Save_Click(object sender, RoutedEventArgs e) => SaveAs();

    private bool SaveAs()
    {
        if (_pixels is null || _loading) return false;
        var directory = _filePath is null ? null : Path.GetDirectoryName(_filePath);
        var name = _filePath is null ? "image" : Path.GetFileNameWithoutExtension(_filePath);
        var dialog = new SaveFileDialog
        {
            Title = "Сохранить изображение",
            Filter = "PNG (*.png)|*.png|JPEG (*.jpg)|*.jpg",
            FilterIndex = 1,
            DefaultExt = ".png",
            AddExtension = true,
            FileName = name + "-edit",
            InitialDirectory = directory ?? Environment.GetFolderPath(Environment.SpecialFolder.MyPictures)
        };
        if (dialog.ShowDialog(this) != true) return false;

        var target = dialog.FileName;
        if (!string.IsNullOrEmpty(_filePath) && Path.GetFullPath(target).Equals(Path.GetFullPath(_filePath), StringComparison.OrdinalIgnoreCase))
        {
            var confirm = MessageBox.Show(this, "Перезаписать исходный файл?", "ThodView",
                MessageBoxButton.YesNo, MessageBoxImage.Question);
            if (confirm != MessageBoxResult.Yes) return false;
        }

        try
        {
            SaveImage(target);
            _dirty = false;
            _filePath = Path.GetFullPath(target);
            RefreshFolderList(_filePath);
            UpdateFolderCounter();
            UpdateFolderNavVisibility();
            UpdateTitle();
            UpdateStatus();
            StatusLeft.Text = "Сохранено: " + Path.GetFileName(target);
            StatusLeft.ToolTip = target;
            ShowGear();
            return true;
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "Не удалось сохранить", MessageBoxButton.OK, MessageBoxImage.Error);
            return false;
        }
    }

    private void SaveImage(string path)
    {
        var bitmap = CreateBitmap();
        BitmapEncoder encoder;
        if (Path.GetExtension(path).ToLowerInvariant() is ".jpg" or ".jpeg")
        {
            var visual = new DrawingVisual();
            using (var context = visual.RenderOpen())
            {
                context.DrawRectangle(Brushes.White, null, new Rect(0, 0, _width, _height));
                context.DrawImage(bitmap, new Rect(0, 0, _width, _height));
            }
            var flattened = new RenderTargetBitmap(_width, _height, 96, 96, PixelFormats.Pbgra32);
            flattened.Render(visual);
            flattened.Freeze();
            encoder = new JpegBitmapEncoder { QualityLevel = 95 };
            encoder.Frames.Add(BitmapFrame.Create(flattened));
        }
        else
        {
            encoder = new PngBitmapEncoder();
            encoder.Frames.Add(BitmapFrame.Create(bitmap));
        }

        using var stream = File.Create(path);
        encoder.Save(stream);
    }

    /* ------------------------------------------------------------------ */
    /* Window chrome                                                       */
    /* ------------------------------------------------------------------ */

    private void TitleBar_MouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ClickCount == 2)
        {
            ToggleMaximize();
            return;
        }
        if (FindAncestor<ButtonBase>(e.OriginalSource as DependencyObject) is not null) return;
        try { DragMove(); } catch { /* released too fast */ }
    }

    private void Minimize_Click(object sender, RoutedEventArgs e) => WindowState = WindowState.Minimized;

    private void Maximize_Click(object sender, RoutedEventArgs e) => ToggleMaximize();

    private void ToggleMaximize()
    {
        WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;
    }

    private void Close_Click(object sender, RoutedEventArgs e) => Close();

    private void Window_StateChanged(object? sender, EventArgs e)
    {
        MaximizeButton.Content = WindowState == WindowState.Maximized ? "\uE923" : "\uE922";
        MaximizeButton.ToolTip = WindowState == WindowState.Maximized ? "Восстановить" : "Развернуть";
    }

    private IntPtr WndProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
    {
        if (msg != WM_GETMINMAXINFO) return IntPtr.Zero;
        var monitor = MonitorFromWindow(hwnd, 2);
        if (monitor == IntPtr.Zero) return IntPtr.Zero;
        var info = new MONITORINFO { cbSize = Marshal.SizeOf<MONITORINFO>() };
        if (!GetMonitorInfo(monitor, ref info)) return IntPtr.Zero;
        var minMax = Marshal.PtrToStructure<MINMAXINFO>(lParam);
        minMax.ptMaxPosition.X = info.rcWork.Left - info.rcMonitor.Left;
        minMax.ptMaxPosition.Y = info.rcWork.Top - info.rcMonitor.Top;
        minMax.ptMaxSize.X = info.rcWork.Right - info.rcWork.Left;
        minMax.ptMaxSize.Y = info.rcWork.Bottom - info.rcWork.Top;
        minMax.ptMinTrackSize.X = (int)MinWidth;
        minMax.ptMinTrackSize.Y = (int)MinHeight;
        Marshal.StructureToPtr(minMax, lParam, true);
        handled = true;
        return IntPtr.Zero;
    }

    private void Window_Closing(object? sender, System.ComponentModel.CancelEventArgs e)
    {
        if (_dirty)
        {
            var result = MessageBox.Show(this, "Есть несохранённые изменения. Закрыть без сохранения?", "ThodView",
                MessageBoxButton.YesNo, MessageBoxImage.Question);
            if (result != MessageBoxResult.Yes) { e.Cancel = true; return; }
        }

        var bounds = WindowState == WindowState.Normal
            ? new Rect(Left, Top, ActualWidth, ActualHeight)
            : RestoreBounds;
        _settings.Width = bounds.Width;
        _settings.Height = bounds.Height;
        _settings.Maximized = WindowState == WindowState.Maximized;
        _settings.Theme = _theme.ToString().ToLowerInvariant();
        _settings.Save(_settingsPath);
        _source?.RemoveHook(WndProc);
    }

    /* ------------------------------------------------------------------ */
    /* Gear / theme                                                        */
    /* ------------------------------------------------------------------ */

    private void Gear_Click(object sender, RoutedEventArgs e)
    {
        if (GearPanel.Visibility == Visibility.Visible)
        {
            GearPanel.Visibility = Visibility.Collapsed;
            _fadeTimer.Start();
        }
        else
        {
            GearPanel.Visibility = Visibility.Visible;
            ShowGear();
        }
    }

    private void Gear_MouseEnter(object sender, MouseEventArgs e) => ShowGear();

    private void Window_MouseMove(object sender, MouseEventArgs e) => ShowGear();

    private void ShowGear()
    {
        _fadeTimer.Stop();
        GearButton.BeginAnimation(OpacityProperty, null);
        GearButton.Opacity = 1;
        _fadeTimer.Start();
    }

    private void Theme_Click(object sender, RoutedEventArgs e)
    {
        _theme = _theme switch
        {
            AppTheme.Auto => AppTheme.Light,
            AppTheme.Light => AppTheme.Dark,
            _ => AppTheme.Auto
        };
        ThemeManager.Apply(_theme);
        ApplyThemeLabel();
        _settings.Theme = _theme.ToString().ToLowerInvariant();
    }

    private void ApplyThemeLabel()
    {
        ThemeLabel.Text = _theme switch
        {
            AppTheme.Light => "Тема: светлая",
            AppTheme.Dark => "Тема: тёмная",
            _ => "Тема: авто"
        };
    }

    private static AppTheme ParseTheme(string? value) => value?.ToLowerInvariant() switch
    {
        "light" => AppTheme.Light,
        "dark" => AppTheme.Dark,
        _ => AppTheme.Auto
    };

    /* ------------------------------------------------------------------ */
    /* File actions                                                        */
    /* ------------------------------------------------------------------ */

    private void OpenFolder_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var target = _filePath;
            var arguments = target is not null && File.Exists(target)
                ? "/select,\"" + target + "\""
                : "\"" + AppContext.BaseDirectory + "\"";
            Process.Start(new ProcessStartInfo("explorer.exe", arguments) { UseShellExecute = true });
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "Не удалось открыть папку", MessageBoxButton.OK, MessageBoxImage.Error);
        }
    }

    private void Metadata_Click(object sender, RoutedEventArgs e)
    {
        if (_filePath is null)
        {
            MessageBox.Show(this, "Сначала откройте изображение.", "Метаданные",
                MessageBoxButton.OK, MessageBoxImage.Information);
            return;
        }
        if (_dirty)
        {
            var choice = MessageBox.Show(this,
                "Есть несохранённые изменения изображения. Метаданные применяются к файлу на диске. Продолжить?",
                "Метаданные", MessageBoxButton.YesNo, MessageBoxImage.Question);
            if (choice != MessageBoxResult.Yes) return;
        }

        GearPanel.Visibility = Visibility.Collapsed;
        try
        {
            var window = new MetadataWindow(_filePath) { Owner = this };
            window.ShowDialog();
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "Метаданные", MessageBoxButton.OK, MessageBoxImage.Error);
        }
    }

    private void DefaultViewer_Click(object sender, RoutedEventArgs e)
    {
        GearPanel.Visibility = Visibility.Collapsed;
        try
        {
            FileAssociationService.Register();
            FileAssociationService.OpenDefaultAppsSettings();
            MessageBox.Show(this,
                "ThodView зарегистрирован в Windows для изображений.\n\n" +
                "В открывшихся настройках Windows выберите ThodView для нужных форматов " +
                "(или нажмите «Выбрать программу по умолчанию»).\n\n" +
                "Windows не разрешает приложению молча назначить себя программой по умолчанию.",
                "Программа по умолчанию", MessageBoxButton.OK, MessageBoxImage.Information);
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, ex.Message, "Не удалось зарегистрировать программу",
                MessageBoxButton.OK, MessageBoxImage.Error);
        }
    }

    /* ------------------------------------------------------------------ */
    /* Commands / status                                                   */
    /* ------------------------------------------------------------------ */

    private void Window_KeyDown(object sender, KeyEventArgs e)
    {
        if (_loading) return;
        var control = (Keyboard.Modifiers & ModifierKeys.Control) != 0;
        if (control && e.Key == Key.O)
        {
            Open_Click(this, new RoutedEventArgs());
            e.Handled = true;
            return;
        }
        if (control && e.Key == Key.S)
        {
            SaveAs();
            e.Handled = true;
            return;
        }
        if (control) return;

        switch (e.Key)
        {
            case Key.Left:
                if (!_cropMode) _ = SwitchImageAsync(-1);
                e.Handled = true;
                break;
            case Key.Right:
                if (!_cropMode) _ = SwitchImageAsync(1);
                e.Handled = true;
                break;
            case Key.F:
                OpenFolder_Click(this, new RoutedEventArgs());
                e.Handled = true;
                break;
            case Key.M:
                Metadata_Click(this, new RoutedEventArgs());
                e.Handled = true;
                break;
            case Key.R:
                Rotate((Keyboard.Modifiers & ModifierKeys.Shift) == 0);
                e.Handled = true;
                break;
            case Key.H:
                Flip(true);
                e.Handled = true;
                break;
            case Key.V:
                Flip(false);
                e.Handled = true;
                break;
            case Key.C:
                ToggleCrop();
                e.Handled = true;
                break;
            case Key.D0:
            case Key.NumPad0:
                FitToWindow();
                e.Handled = true;
                break;
            case Key.D1:
            case Key.NumPad1:
                ActualSize();
                e.Handled = true;
                break;
            case Key.OemPlus:
            case Key.Add:
                ZoomCenter(1.25);
                e.Handled = true;
                break;
            case Key.OemMinus:
            case Key.Subtract:
                ZoomCenter(1 / 1.25);
                e.Handled = true;
                break;
            case Key.T:
                Theme_Click(this, new RoutedEventArgs());
                e.Handled = true;
                break;
            case Key.Escape:
                if (_cropMode) ExitCrop();
                else if (GearPanel.Visibility == Visibility.Visible) GearPanel.Visibility = Visibility.Collapsed;
                e.Handled = true;
                break;
        }
    }

    private void Window_DragOver(object sender, DragEventArgs e)
    {
        e.Effects = e.Data.GetDataPresent(DataFormats.FileDrop) ? DragDropEffects.Copy : DragDropEffects.None;
        e.Handled = true;
    }

    private void Window_Drop(object sender, DragEventArgs e)
    {
        if (e.Data.GetData(DataFormats.FileDrop) is string[] files && files.Length > 0)
        {
            _ = OpenFileAsync(files[0]);
        }
    }

    private void UpdateTitle()
    {
        var name = _filePath is null ? "Ничего не открыто" : Path.GetFileName(_filePath);
        TitleText.Text = _dirty ? name + " •" : name;
        TitleText.ToolTip = _filePath;
        Title = _dirty ? "* " + name + " — ThodView" : name + " — ThodView";
    }

    private void UpdateStatus()
    {
        var hasImage = _pixels is not null;
        EditToggleButton.Visibility = hasImage ? Visibility.Visible : Visibility.Collapsed;
        OpenFolderButton.IsEnabled = _filePath is not null;
        if (!hasImage)
        {
            _toolbarOpen = false;
            EditToolbar.Visibility = Visibility.Collapsed;
            ToolbarTransform.X = -18;
            EditToolbar.Opacity = 1;
            StatusLeft.Text = "Готово";
            StatusCenter.Text = "";
            StatusRight.Text = "—";
            FolderNav.Visibility = Visibility.Collapsed;
            return;
        }
        StatusLeft.Text = _filePath is null ? "Изображение" : Path.GetFileName(_filePath);
        StatusLeft.ToolTip = _filePath;
        StatusCenter.Text = $"{_width} × {_height}";
        UpdateFolderCounter();
        UpdateZoomText();
    }

    private void UpdateZoomText()
    {
        StatusRight.Text = Math.Round(_scale * 100) + "%";
    }

    private void CleanCache()
    {
        try
        {
            var cache = Path.Combine(_dataDir, "cache");
            if (!Directory.Exists(cache)) return;
            foreach (var file in Directory.EnumerateFiles(cache))
            {
                try
                {
                    if (File.GetLastWriteTimeUtc(file) < DateTime.UtcNow.AddDays(-7)) File.Delete(file);
                }
                catch { }
            }
        }
        catch { }
    }

    /* ------------------------------------------------------------------ */
    /* Helpers                                                             */
    /* ------------------------------------------------------------------ */

    private static T? FindAncestor<T>(DependencyObject? current) where T : DependencyObject
    {
        while (current is not null)
        {
            if (current is T match) return match;
            try { current = VisualTreeHelper.GetParent(current); }
            catch { return null; }
        }
        return null;
    }

    private static bool IsInside(DependencyObject? source, DependencyObject container)
    {
        while (source is not null)
        {
            if (ReferenceEquals(source, container)) return true;
            try { source = VisualTreeHelper.GetParent(source); }
            catch { return false; }
        }
        return false;
    }

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint flags);

    [DllImport("user32.dll")]
    private static extern bool GetMonitorInfo(IntPtr monitor, ref MONITORINFO info);

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT
    {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct RECT
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MINMAXINFO
    {
        public POINT ptReserved;
        public POINT ptMaxSize;
        public POINT ptMaxPosition;
        public POINT ptMinTrackSize;
        public POINT ptMaxTrackSize;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MONITORINFO
    {
        public int cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
    }
}

