# Firma una release de Centro LED: crea SHA256SUMS.txt y SHA256SUMS.txt.sig junto a los .exe.
#
# Uso:  powershell -ExecutionPolicy Bypass -File tools\firmar-release.ps1 <carpeta con los .exe>   (o "clave" para crear o ver la clave)
#
# La clave privada NO esta en el repositorio: se guarda en %USERPROFILE%\.centroled\clave-firma.bin
# (se crea la primera vez). Haz una copia de seguridad: sin ella no se pueden firmar versiones nuevas
# y las copias instaladas rechazaran cualquier actualizacion firmada con otra clave.
# Es una clave ECDSA P-256 generada en tu PC: gratis, no hace falta comprar ningun certificado.
param([Parameter(Mandatory = $true)][string]$Carpeta)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Core
$C = 'System.Security.Cryptography'

$dirClave = Join-Path $env:USERPROFILE '.centroled'
$clave = Join-Path $dirClave 'clave-firma.bin'
if (-not (Test-Path $clave)) {
    New-Item -ItemType Directory -Force $dirClave | Out-Null
    $p = New-Object "$C.CngKeyCreationParameters"
    $p.ExportPolicy = [System.Security.Cryptography.CngExportPolicies]::AllowPlaintextExport
    $k = [System.Security.Cryptography.CngKey]::Create([System.Security.Cryptography.CngAlgorithm]::ECDsaP256, $null, $p)
    [IO.File]::WriteAllBytes($clave, $k.Export([System.Security.Cryptography.CngKeyBlobFormat]::EccPrivateBlob))
    Write-Host "Clave nueva creada en $clave  (haz una copia de seguridad)"
}
$k = [System.Security.Cryptography.CngKey]::Import([IO.File]::ReadAllBytes($clave), [System.Security.Cryptography.CngKeyBlobFormat]::EccPrivateBlob)
$pub = $k.Export([System.Security.Cryptography.CngKeyBlobFormat]::EccPublicBlob)
Write-Host ('Clave publica (PUBLIC_KEY en src/update.rs): ' + (($pub[8..71] | ForEach-Object { $_.ToString('x2') }) -join ''))

if ($Carpeta -eq 'clave') { return }   # solo crear o mostrar la clave

$sha = [System.Security.Cryptography.SHA256]::Create()
$lineas = foreach ($n in 'CentroLED.exe', 'CentroLED-Portable.exe', 'CentroLED-Setup.exe') {
    $f = Join-Path $Carpeta $n
    if (Test-Path $f) { (($sha.ComputeHash([IO.File]::ReadAllBytes($f)) | ForEach-Object { $_.ToString('x2') }) -join '') + '  ' + $n }
}
$bytes = [Text.Encoding]::ASCII.GetBytes((($lineas -join "`n") + "`n"))
[IO.File]::WriteAllBytes((Join-Path $Carpeta 'SHA256SUMS.txt'), $bytes)
$ec = New-Object "$C.ECDsaCng" -ArgumentList $k
$ec.HashAlgorithm = [System.Security.Cryptography.CngAlgorithm]::Sha256
$firma = $ec.SignData($bytes)
[IO.File]::WriteAllText((Join-Path $Carpeta 'SHA256SUMS.txt.sig'), (($firma | ForEach-Object { $_.ToString('x2') }) -join ''))
Write-Host "Firmado: $Carpeta\SHA256SUMS.txt y SHA256SUMS.txt.sig"
