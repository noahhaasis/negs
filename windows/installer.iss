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
AppVersion=0.12.0
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
AppMutex=Local\Negs.AppLifetime,Local\Negs.SingleInstance
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

[Code]
var
  KeepSettings: Boolean;
  SettingsChoice: String;
  CleanupFailures: String;

function InitializeUninstall: Boolean;
begin
  { Only actual uninstall runs this code. Installing an update keeps all data. }
  SettingsChoice := ExpandConstant('{param:KEEPSETTINGS|ask}');
  Result := (SettingsChoice = 'ask') or (SettingsChoice = '0') or (SettingsChoice = '1');
  if not Result then
    SuppressibleMsgBox('KEEPSETTINGS must be 0 or 1.', mbError, MB_OK, IDOK);
  KeepSettings := SettingsChoice = '1';
end;

procedure CleanupFailed(const Path: String);
begin
  Log('Could not remove app data: ' + Path);
  CleanupFailures := CleanupFailures + Path + #13#10;
end;

procedure DeleteAppDataFile(const Path: String);
begin
  if FileExists(Path) then
    if not DeleteFile(Path) then CleanupFailed(Path);
end;

procedure DeleteAppDataDirectory(const Path: String);
begin
  if DirExists(Path) then
    if not DelTree(Path, True, True, True) then CleanupFailed(Path);
end;

procedure RemoveAppData;
var
  LocalData, RoamingData: String;
begin
  LocalData := ExpandConstant('{localappdata}\Negs');
  RoamingData := ExpandConstant('{userappdata}\Negs');
  { Fixed, app-owned targets. Never read paths from session.json or delete
    everything in the install directory, which may contain user photographs. }
  DeleteAppDataFile(LocalData + '\catalog.json');
  DeleteAppDataFile(LocalData + '\catalog.tmp');
  DeleteAppDataFile(LocalData + '\rendered-preview.cache');
  DeleteAppDataFile(LocalData + '\rendered-preview.tmp');
  DeleteAppDataDirectory(LocalData + '\Cache');
  DeleteAppDataFile(RoamingData + '\panic.log');
  DeleteAppDataFile(RoamingData + '\settings.tmp');
  DeleteAppDataFile(RoamingData + '\session.tmp');
  DeleteAppDataDirectory(GetTempDir + 'Negs');
  if not KeepSettings then
  begin
    DeleteAppDataFile(RoamingData + '\settings.json');
    DeleteAppDataFile(RoamingData + '\session.json');
    DeleteAppDataDirectory(RoamingData + '\Presets');
    if RegKeyExists(HKCU, 'Software\Negs') then
      if not RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Negs') then
        CleanupFailed('HKCU\Software\Negs');
  end;
  { Leave any unrelated files alone; remove only empty parent directories. }
  RemoveDir(LocalData);
  RemoveDir(RoamingData);
  if CleanupFailures <> '' then
    SuppressibleMsgBox('Negs was uninstalled, but some app data could not be removed. Close any other running copy and remove these items manually:' + #13#10#13#10 + CleanupFailures,
      mbError, MB_OK, IDOK);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Choice: Integer;
begin
  if CurUninstallStep = usUninstall then
  begin
    if (not UninstallSilent) and (SettingsChoice = 'ask') then
    begin
      Choice := TaskDialogMsgBox('Choose what to keep after uninstalling Negs',
        'Caches, logs, and temporary files will be removed either way. Original photographs and exported images are kept.',
        mbConfirmation, MB_YESNOCANCEL, ['Remove all app data', 'Keep settings, presets and editing session', 'Cancel'], 0);
      if (Choice <> IDYES) and (Choice <> IDNO) then Abort;
      KeepSettings := Choice = IDNO;
    end;
  end;
  if CurUninstallStep = usPostUninstall then RemoveAppData;
end;
