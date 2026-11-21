$root = 'F:\Deepseek\Research\luma'
$zip = Join-Path $root 'dist\Luma-0.3.1-win64.zip'
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $root 'dist\Luma.exe'), (Join-Path $root 'dist\README.md') -DestinationPath $zip -CompressionLevel Optimal
icacls $zip /setintegritylevel Medium | Out-Null
icacls (Join-Path $root 'dist\Luma.exe') /setintegritylevel Medium | Out-Null
