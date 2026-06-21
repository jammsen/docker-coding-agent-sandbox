<#
.SYNOPSIS
    Saves a screenshot from the Windows clipboard into the sandbox workspace.

.DESCRIPTION
    Reads an image from the Windows clipboard (e.g. captured with Win+Shift+S or PrtScn)
    and saves it to .\workspace\uploads\ with a timestamp filename.

    The saved path is printed as the container-side path so you can paste it
    directly into the agent prompt.

.EXAMPLE
    # 1. Take a screenshot with Win+Shift+S  (image goes to clipboard)
    # 2. Run this script:
    .\scripts\paste-screenshot.ps1
    # 3. Copy the printed path and tell the agent:
    #    "There is a screenshot at /home/agent/workspace/uploads/screenshot-20260621-143022.png"
#>

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$image = [System.Windows.Forms.Clipboard]::GetImage()

if ($null -eq $image) {
    Write-Error "No image found in clipboard. Take a screenshot first (Win+Shift+S or PrtScn)."
    exit 1
}

# Ensure upload directory exists
$uploadDir = Join-Path $PSScriptRoot "..\workspace\uploads"
$uploadDir = [System.IO.Path]::GetFullPath($uploadDir)
if (-not (Test-Path $uploadDir)) {
    New-Item -ItemType Directory -Path $uploadDir | Out-Null
}

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$filename  = "screenshot-$timestamp.png"
$fullPath  = Join-Path $uploadDir $filename

$image.Save($fullPath, [System.Drawing.Imaging.ImageFormat]::Png)
$image.Dispose()

# Print the container-side path (workspace is mounted at /home/agent/workspace)
$containerPath = "/home/agent/workspace/uploads/$filename"

Write-Host ""
Write-Host "Screenshot saved." -ForegroundColor Green
Write-Host ""
Write-Host "Tell the agent:" -ForegroundColor Cyan
Write-Host "  $containerPath" -ForegroundColor White
Write-Host ""
