# DRAFT: fill after first release. Not submitted. Replace every FILL-AFTER-FIRST-RELEASE.
$ErrorActionPreference = 'Stop'
$toolsDir = "$(Split-Path -Parent $MyInvocation.MyCommand.Definition)"
$version  = 'FILL-AFTER-FIRST-RELEASE'

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  unzipLocation  = $toolsDir
  url64bit       = "https://github.com/ChadaSaiteja/SpaceOps/releases/download/v$version/SpaceOps-win-x64.zip"
  checksum64     = 'FILL-AFTER-FIRST-RELEASE'
  checksumType64 = 'sha256'
}
Install-ChocolateyZipPackage @packageArgs
