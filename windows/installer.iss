; Compile with Inno Setup 6 through package.ps1 -Iscc PATH.
; File associations are alternate Open With entries, never defaults.
#ifndef StageDir
  #error StageDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
[Setup]
AppId=dev.negs.app
AppName=Negs
AppVersion=0.7.0
AppPublisher=Negs
DefaultDirName={localappdata}\Programs\Negs
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=Negs-Setup
Compression=lzma2
SolidCompression=yes
UninstallDisplayIcon={app}\Negs.exe
ChangesAssociations=yes
CloseApplications=yes
LicenseFile={#StageDir}\LICENSE
[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
[Icons]
Name: "{userprograms}\Negs"; Filename: "{app}\Negs.exe"; AppUserModelID: "dev.negs.app"
[Registry]
Root: HKCU; Subkey: "Software\Classes\Negs.Photograph"; ValueType: string; ValueData: "Photograph"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Negs.Photograph\DefaultIcon"; ValueType: string; ValueData: "{app}\Negs.exe,0"
Root: HKCU; Subkey: "Software\Classes\Negs.Photograph\shell\open\command"; ValueType: string; ValueData: """{app}\Negs.exe"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\Applications\Negs.exe\shell\open\command"; ValueType: string; ValueData: """{app}\Negs.exe"" ""%1"""; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Negs\Capabilities"; ValueName: "ApplicationName"; ValueType: string; ValueData: "Negs"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Negs\Capabilities"; ValueName: "ApplicationDescription"; ValueType: string; ValueData: "Film photography editor"
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueName: "Negs"; ValueType: string; ValueData: "Software\Negs\Capabilities"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.jpg\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".jpg"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.jpeg\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".jpeg"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.png\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".png"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.tif\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".tif"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.tiff\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".tiff"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.dng\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".dng"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.cr2\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".cr2"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.cr3\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".cr3"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.nef\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".nef"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.nrw\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".nrw"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.arw\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".arw"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.srf\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".srf"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.sr2\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".sr2"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.raf\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".raf"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.orf\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".orf"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.rw2\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".rw2"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.pef\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".pef"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.ptx\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".ptx"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.srw\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".srw"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.x3f\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".x3f"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.iiq\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".iiq"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.3fr\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".3fr"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.ari\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".ari"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.bay\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".bay"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.crw\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".crw"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.dcr\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".dcr"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.drf\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".drf"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.erf\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".erf"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.fff\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".fff"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.k25\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".k25"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.kdc\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".kdc"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.mef\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".mef"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.mos\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".mos"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.mrw\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".mrw"; ValueType: string; ValueData: "Negs.Photograph"
Root: HKCU; Subkey: "Software\Classes\.rwl\OpenWithProgids"; ValueName: "Negs.Photograph"; ValueType: string; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Negs\Capabilities\FileAssociations"; ValueName: ".rwl"; ValueType: string; ValueData: "Negs.Photograph"
