#ifndef MyAppConfig
#define MyAppConfig "regtest-lan"
#endif

#if MyAppConfig == "testnet"
#define MyAppName "Qubit Coin Core Testnet"
#define MyAppDirName "Qubit Coin Core Testnet"
#define MyAppGroupName "Qubit Coin Core Testnet"
#define MyAppShortcutName "Qubit Coin Core Testnet"
#define MyAppId "{{2D0E5C31-5E3E-4B1C-9C9C-0B3F3A000110}"
#else
#define MyAppName "Qubit Coin Core"
#define MyAppDirName "Qubit Coin Core"
#define MyAppGroupName "Qubit Coin Core"
#define MyAppShortcutName "Qubit Coin Core"
#define MyAppId "{{8E24D6B6-51BE-4C5C-9C01-0B3F3A000100}"
#endif

#define MyAppVersion "1.7.2"
#define MyAppPublisher "Alexander Proestakis"
#ifndef MyAppSource
#define MyAppSource "..\dist\QUB-Core-v1.7.2-windows-x64-regtest-lan"
#endif

[Setup]
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\{#MyAppDirName}
DefaultGroupName={#MyAppGroupName}
DisableProgramGroupPage=yes
AppVerName={#MyAppName} {#MyAppVersion}
VersionInfoVersion={#MyAppVersion}
VersionInfoProductVersion={#MyAppVersion}
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} Installer
SetupLogging=yes
CloseApplications=yes
RestartApplications=no
OutputDir=..\dist\installer
OutputBaseFilename=QUB-Core-v{#MyAppVersion}-Windows-x64-{#MyAppConfig}-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
UninstallDisplayIcon={app}\QUB-Core.exe

[Files]
Source: "{#MyAppSource}\QUB-Core.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#MyAppSource}\README-MINER-WINDOWS.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#MyAppSource}\SHA256SUMS.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#MyAppSource}\config\*"; DestDir: "{app}\config"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#MyAppSource}\assets\*"; DestDir: "{app}\assets"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#MyAppSource}\tools\*"; DestDir: "{app}\tools"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppShortcutName}"; Filename: "{app}\QUB-Core.exe"; Tasks: startmenuicon
Name: "{autodesktop}\{#MyAppShortcutName}"; Filename: "{app}\QUB-Core.exe"; Tasks: desktopicon

[Tasks]
Name: "startmenuicon"; Description: "Create a Start menu shortcut"; GroupDescription: "Shortcuts:"; Flags: checkedonce
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Run]
Filename: "{app}\QUB-Core.exe"; Description: "Run {#MyAppName}"; Flags: nowait postinstall skipifsilent unchecked
