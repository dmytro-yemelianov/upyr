#ifndef AppVersion
  #error AppVersion must be supplied from Cargo metadata
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\release"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
AppId={{487BE3CD-4C0A-40B8-B496-0A7DA0D74858}
AppName=Upyr
AppVersion={#AppVersion}
AppPublisher=Upyr contributors
AppPublisherURL=https://dmytro-yemelianov.github.io/upyr/
AppSupportURL=https://github.com/dmytro-yemelianov/upyr/issues
AppUpdatesURL=https://github.com/dmytro-yemelianov/upyr/releases
DefaultDirName={localappdata}\Programs\Upyr
DefaultGroupName=Upyr
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=upyr-windows-x86_64-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
UninstallDisplayIcon={app}\upyr-background.exe
UninstallDisplayName=Upyr {#AppVersion}
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#AppVersion}
VersionInfoCompany=Upyr contributors
VersionInfoDescription=Private English-Ukrainian keyboard layout fixer
VersionInfoProductName=Upyr
VersionInfoProductVersion={#AppVersion}
VersionInfoCopyright=Copyright (C) 2026 Upyr contributors

[Tasks]
Name: "autostart"; Description: "Launch Upyr when I sign in"; GroupDescription: "Startup:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\upyr-background.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\upyr.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\upyr-settings.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\..\..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Upyr"; Filename: "{app}\upyr-background.exe"
Name: "{group}\Upyr diagnostics"; Filename: "{app}\upyr.exe"; Parameters: "doctor"
Name: "{group}\Upyr Settings"; Filename: "{app}\upyr-settings.exe"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Upyr"; ValueData: """{app}\upyr-background.exe"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\upyr-background.exe"; Description: "Launch Upyr"; Flags: postinstall nowait skipifsilent
