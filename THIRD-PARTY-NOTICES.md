# Third-party notices — ThodView

ThodView itself is licensed under the MIT License (see `LICENSE`).
The package bundles the components below. Full license texts are in `licenses/`.

## ffmpeg 9.0.2 essentials build

- Project: https://ffmpeg.org/
- Build: Gyan Doshi, https://www.gyan.dev/ffmpeg/builds/ (essentials, static, win-x64)
- License: GNU General Public License v3 (the build is configured with `--enable-gpl --enable-version3`)
- Binaries in package: `app/runtime/ffmpeg/ffmpeg.exe`, `app/runtime/ffmpeg/ffprobe.exe`
- License text: `licenses/ffmpeg-COPYING.txt` (copied from the official build archive)
- Corresponding source: `licenses/FFMPEG-SOURCE-OFFER.txt`

ThodView invokes ffmpeg as a separate process through its command-line interface; no ffmpeg code is linked into ThodView. Because the bundled build is GPLv3, the package as a whole is distributed under GPLv3 terms.

## ExifTool 13.59

- Project: https://exiftool.org/
- Windows package: https://oliverbetz.de/pages/Artikel/ExifTool-for-Windows
- Author: Phil Harvey
- License: GNU General Public License v3
- Binaries in package: `app/runtime/exiftool/ExifTool.exe`, `app/runtime/exiftool/exiftool_files/`
- License text: `licenses/exiftool-COPYING.txt`
- Source: https://exiftool.org/ and https://github.com/exiftool/exiftool

ThodView invokes ExifTool as a separate process to read, edit and remove image metadata. No ExifTool code is linked into ThodView.

## .NET 10 runtime and Windows Desktop libraries

- Project: https://github.com/dotnet/runtime and https://github.com/dotnet/wpf
- License: MIT
- The self-contained application includes the official Microsoft runtime files.

## Icons and artwork

Application icon and artwork are original works of the ThodProd project, MIT.
