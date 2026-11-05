param(
    [Parameter(Mandatory=$true)][string]$Command,
    [Parameter(Mandatory=$true)][string]$Log,
    [string]$Wd = ""
)
if ($Wd -ne "") { Push-Location $Wd }
$exitCode = 0
$sw = [System.IO.StreamWriter]::new($Log, $false, [System.Text.Encoding]::UTF8)
try {
    cmd /c "$Command" 2>&1 | ForEach-Object { Write-Host $_; $sw.WriteLine($_); $sw.Flush() }
    $exitCode = $LASTEXITCODE
} catch {
    Write-Host 'ERR:' $_.Exception.Message
    $exitCode = 1
} finally {
    $sw.Close()
}
if ($Wd -ne "") { Pop-Location }
exit $exitCode
