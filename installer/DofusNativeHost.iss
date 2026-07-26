#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif

#define MyAppName "Dofus 3 Native Host"
#define MyAppPublisher "Nexytrus contributors"
#define MyAppUrl "https://github.com/AlpaGit/dofus3-native-host"

[Setup]
AppId={{8D78508E-F9BB-4D95-A90A-67C055C9146A}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppUrl}
AppSupportURL={#MyAppUrl}/issues
AppUpdatesURL={#MyAppUrl}/releases/latest
DefaultDirName={localappdata}\Ankama\Dofus-dofus3
DisableProgramGroupPage=yes
DisableWelcomePage=no
DirExistsWarning=no
LicenseFile=..\LICENSE
OutputDir=output
OutputBaseFilename=Dofus3NativeHost-Setup-windows-x64
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
RestartApplications=no
UninstallDisplayName={#MyAppName}
UninstallDisplayIcon={app}\Dofus.exe
VersionInfoVersion={#MyAppVersion}
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription=Dofus 3 native mod host installer

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "..\dist\DofusRuntime\version.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\DofusRuntime\DofusNativeBootstrap.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\DofusRuntime\DofusNativeHost.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\DofusRuntime\NativeMods\DofusNativeExample.dll"; DestDir: "{app}\NativeMods"; Flags: ignoreversion
Source: "..\dist\DofusRuntime\NativeMods\DofusNativeTactical.dll"; DestDir: "{app}\NativeMods"; Flags: ignoreversion
Source: "..\dist\DofusRuntime\NativeMods\DofusCombatAnimationSkipper.dll"; DestDir: "{app}\NativeMods"; Flags: ignoreversion

[UninstallDelete]
Type: files; Name: "{app}\NativeMods\native-control.json"
Type: files; Name: "{app}\NativeMods\native-mods.json"
Type: files; Name: "{app}\NativeMods\native-bootstrap.log"
Type: files; Name: "{app}\NativeMods\native-host.log"
Type: filesandordirs; Name: "{app}\NativeMods\control"

[Code]
function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = wpSelectDir then
  begin
    if not FileExists(AddBackslash(WizardDirValue) + 'Dofus.exe') then
    begin
      MsgBox(
        'Sélectionnez le dossier Dofus 3 contenant Dofus.exe.' + #13#10 +
        'Choose the Dofus 3 folder that contains Dofus.exe.',
        mbError,
        MB_OK);
      Result := False;
    end;
  end;
end;

function InitializeSetup(): Boolean;
begin
  Result := True;
  if CheckForMutexes('DofusNativeHostInstaller') then
  begin
    MsgBox(
      'Une autre installation est déjà en cours.' + #13#10 +
      'Another installation is already running.',
      mbError,
      MB_OK);
    Result := False;
  end
  else
    CreateMutex('DofusNativeHostInstaller');
end;
