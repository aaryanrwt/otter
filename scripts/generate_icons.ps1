Add-Type -AssemblyName System.Drawing
$src = "C:\Users\Aaryan Rawat\.gemini\antigravity\brain\142d265b-4d47-4423-b08b-66e213b68c71\.user_uploaded\media_1794074794480_35b5b2bb.jpg"
$img = [System.Drawing.Image]::FromFile($src)

function Resize-Save($sourceImg, $size, $destPath) {
    $bmp = New-Object System.Drawing.Bitmap($size, $size)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.DrawImage($sourceImg, 0, 0, $size, $size)
    $bmp.Save($destPath, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose()
    $bmp.Dispose()
}

$targets = @("extensions\chrome\icons", "extensions\firefox\icons")
foreach ($folder in $targets) {
    if (-not (Test-Path $folder)) {
        New-Item -ItemType Directory -Force -Path $folder | Out-Null
    }
    Resize-Save $img 16 (Join-Path $folder "icon16.png")
    Resize-Save $img 48 (Join-Path $folder "icon48.png")
    Resize-Save $img 128 (Join-Path $folder "icon128.png")
    Resize-Save $img 256 (Join-Path $folder "mascot.png")
    Write-Output "Successfully generated all icons and mascot in $folder"
}
$img.Dispose()
