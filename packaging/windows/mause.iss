#ifndef AppVersion
  #error Build with Build-Installer.ps1
#endif
#ifndef VcVersion
  #error The tested VC runtime version is required
#endif
#ifndef RepositoryUrl
  #define RepositoryUrl "https://github.com"
#endif

[Setup]
AppId={{B840A882-6C48-4E4D-98A9-469B86A19FC2}
AppName=Mause
AppVersion={#AppVersion}
AppPublisher=Mause
AppPublisherURL={#RepositoryUrl}
AppSupportURL={#RepositoryUrl}/issues
AppUpdatesURL={#RepositoryUrl}/releases
DefaultDirName={autopf}\Mause
DefaultGroupName=Mause
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0.18362
PrivilegesRequired=admin
WizardStyle=modern dark includetitlebar
DisableWelcomePage=no
WizardImageFile=wizard-large.png
WizardSmallImageFile=wizard-small.png
AppCopyright=Muhammed Taymur, Serdar Kayışkıran
SetupIconFile={#PayloadDir}\mause.ico
UninstallDisplayIcon={app}\mause.ico
ChangesAssociations=yes
OutputDir={#OutputDir}
OutputBaseFilename=mause-v{#AppVersion}-windows-x86_64-setup
VersionInfoVersion={#AppVersion}
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
CloseApplicationsFilter=mause.exe
RestartApplications=no
SetupMutex=MauseInstaller
ChangesEnvironment=yes
UsePreviousAppDir=yes
UsePreviousSetupType=yes
UsePreviousTasks=yes
InfoBeforeFile={#PayloadDir}\THIRD_PARTY.md

[LangOptions]
DialogFontSize=11
WelcomeFontSize=17

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"

[CustomMessages]
english.TypeFull=Mause + USB sharing drivers
turkish.TypeFull=Mause + USB paylaşım sürücüleri
english.TypeCompact=Mause without USB sharing drivers
turkish.TypeCompact=USB paylaşım sürücüleri olmadan Mause
english.TypeCustom=Custom installation
turkish.TypeCustom=Özel kurulum
english.ComponentCore=Mause and required Visual C++ runtime
turkish.ComponentCore=Mause ve gerekli Visual C++ çalışma zamanı
english.ComponentUsb=USB sharing (usbipd-win server + USBip client drivers)
turkish.ComponentUsb=USB paylaşımı (usbipd-win sunucusu + USBip istemci sürücüleri)
english.TaskFirewall=Allow Mause connections on private networks
turkish.TaskFirewall=Özel ağlarda Mause bağlantılarına izin ver
english.TaskDesktop=Create a desktop shortcut
turkish.TaskDesktop=Masaüstü kısayolu oluştur
english.LaunchMause=Launch Mause
turkish.LaunchMause=Mause'u başlat
english.DevelopersTitle=Developers
turkish.DevelopersTitle=Geliştirenler
english.DevelopersDescription=The people behind Mause
turkish.DevelopersDescription=Mause'u geliştiren ekip
english.DevelopersIntro=Mause is an open-source software KVM: one keyboard and mouse for all your computers.
turkish.DevelopersIntro=Mause açık kaynaklı bir yazılım KVM'dir: tüm bilgisayarlarınız için tek klavye ve fare.
english.DevelopersProject=Project on GitHub
turkish.DevelopersProject=Proje sayfası (GitHub)
english.DevelopedBy=Developed by Muhammed Taymur and Serdar Kayışkıran.
turkish.DevelopedBy=Geliştirenler: Muhammed Taymur ve Serdar Kayışkıran.

[Types]
Name: "full"; Description: "{cm:TypeFull}"
Name: "compact"; Description: "{cm:TypeCompact}"
Name: "custom"; Description: "{cm:TypeCustom}"; Flags: iscustom

[Components]
Name: "core"; Description: "{cm:ComponentCore}"; Types: full compact custom; Flags: fixed
Name: "usb"; Description: "{cm:ComponentUsb}"; Types: full

[Tasks]
Name: "privatefirewall"; Description: "{cm:TaskFirewall}"; Flags: checkedonce
Name: "desktopicon"; Description: "{cm:TaskDesktop}"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\mause.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\mause.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\THIRD_PARTY.md"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#PayloadDir}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#DepsDir}\vc_redist.x64.exe"; Flags: dontcopy
Source: "avatar-mt.bmp"; Flags: dontcopy
Source: "avatar-sk.bmp"; Flags: dontcopy
Source: "{#DepsDir}\usbipd-win_x64.msi"; Flags: dontcopy
Source: "{#DepsDir}\usbip-win2_x64.exe"; Flags: dontcopy

[Icons]
Name: "{group}\Mause"; Filename: "{app}\mause.exe"; IconFilename: "{app}\mause.ico"; WorkingDir: "{app}"
Name: "{autodesktop}\Mause"; Filename: "{app}\mause.exe"; IconFilename: "{app}\mause.ico"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
Root: HKLM; Subkey: "Software\Mause\Installer"; ValueType: string; ValueName: "InstallDir"; ValueData: "{app}"; Flags: uninsdeletevalue uninsdeletekeyifempty

[Run]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Mause (TCP)"" program=""{app}\mause.exe"""; Flags: runhidden; Tasks: privatefirewall
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Mause (UDP)"" program=""{app}\mause.exe"""; Flags: runhidden; Tasks: privatefirewall
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Mause (TCP)"" dir=in action=allow program=""{app}\mause.exe"" profile=private protocol=TCP enable=yes"; Flags: runhidden; Tasks: privatefirewall
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Mause (UDP)"" dir=in action=allow program=""{app}\mause.exe"" profile=private protocol=UDP enable=yes"; Flags: runhidden; Tasks: privatefirewall
Filename: "{app}\mause.exe"; Description: "{cm:LaunchMause}"; Flags: nowait postinstall skipifsilent runasoriginaluser; Check: CanLaunch
Filename: "{app}\mause.exe"; Flags: nowait runasoriginaluser; Check: LaunchAfterUpdate

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Mause (TCP)"" program=""{app}\mause.exe"""; Flags: runhidden; RunOnceId: "RemoveMauseTCP"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Mause (UDP)"" program=""{app}\mause.exe"""; Flags: runhidden; RunOnceId: "RemoveMauseUDP"

[Code]
const
  CardColor = $363636;
  MutedColor = $A8A8A8;

var
  PrerequisiteRestart: Boolean;
  DeveloperLinks: array of String;

procedure OpenDeveloperLink(Sender: TObject);
var ErrorCode: Integer;
begin
  ShellExecAsOriginalUser('open', DeveloperLinks[TComponent(Sender).Tag], '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode);
end;

function AddText(Parent: TWinControl; const Caption: String; Left, Top: Integer): TNewStaticText;
begin
  Result := TNewStaticText.Create(WizardForm);
  Result.Parent := Parent;
  Result.Caption := Caption;
  Result.Left := Left;
  Result.Top := Top;
end;

function AddButton(Parent: TWinControl; const Caption, Url: String; Left, Top, Width: Integer): TNewButton;
var Index: Integer;
begin
  Result := TNewButton.Create(WizardForm);
  Result.Parent := Parent;
  Result.Caption := Caption;
  Result.SetBounds(Left, Top, Width, ScaleY(28));
  Index := GetArrayLength(DeveloperLinks);
  SetArrayLength(DeveloperLinks, Index + 1);
  DeveloperLinks[Index] := Url;
  Result.Tag := Index;
  Result.OnClick := @OpenDeveloperLink;
end;

procedure AddDeveloperCard(Page: TWizardPage; const Name, Avatar, GitHub, LinkedIn: String; Left, Top, Width, Height: Integer);
var Card: TPanel; Picture: TBitmapImage; Title, Handle: TNewStaticText; Pad, TextLeft, ButtonWidth: Integer;
begin
  Pad := ScaleX(16);
  Card := TPanel.Create(WizardForm);
  Card.Parent := Page.Surface;
  Card.SetBounds(Left, Top, Width, Height);
  Card.BevelOuter := bvNone;
  Card.ParentBackground := False;
  Card.StyleElements := [];
  Card.Color := CardColor;
  Picture := TBitmapImage.Create(WizardForm);
  Picture.Parent := Card;
  Picture.SetBounds(Pad, ScaleY(16), ScaleX(48), ScaleY(48));
  Picture.Stretch := True;
  Picture.Bitmap.LoadFromFile(ExpandConstant('{tmp}\' + Avatar));
  TextLeft := Picture.Left + Picture.Width + ScaleX(12);
  Title := AddText(Card, Name, TextLeft, ScaleY(18));
  Title.StyleElements := [];
  Title.Font.Color := clWhite;
  Title.Font.Style := [fsBold];
  Title.Font.Size := Title.Font.Size + 1;
  Handle := AddText(Card, '@' + GitHub, TextLeft, Title.Top + Title.Height + ScaleY(2));
  Handle.StyleElements := [];
  Handle.Font.Color := MutedColor;
  ButtonWidth := (Width - Pad * 2 - ScaleX(8)) div 2;
  AddButton(Card, 'GitHub', 'https://github.com/' + GitHub, Pad, Height - Pad - ScaleY(28), ButtonWidth);
  AddButton(Card, 'LinkedIn', LinkedIn, Pad + ButtonWidth + ScaleX(8), Height - Pad - ScaleY(28), ButtonWidth);
end;

procedure CreateDevelopersPage();
var Page: TWizardPage; Intro: TNewStaticText; Top, Gap, CardWidth, CardHeight: Integer;
begin
  ExtractTemporaryFile('avatar-mt.bmp');
  ExtractTemporaryFile('avatar-sk.bmp');
  Page := CreateCustomPage(wpWelcome, CustomMessage('DevelopersTitle'), CustomMessage('DevelopersDescription'));
  Intro := AddText(Page.Surface, CustomMessage('DevelopersIntro'), 0, 0);
  Intro.AutoSize := False;
  Intro.WordWrap := True;
  Intro.Width := Page.SurfaceWidth;
  Intro.Height := ScaleY(40);
  Top := Intro.Top + Intro.Height + ScaleY(14);
  Gap := ScaleX(14);
  CardWidth := (Page.SurfaceWidth - Gap) div 2;
  CardHeight := ScaleY(132);
  AddDeveloperCard(Page, 'Muhammed Taymur', 'avatar-mt.bmp', 'ByTaymur', 'https://www.linkedin.com/in/muhammed-t-aaa770281/', 0, Top, CardWidth, CardHeight);
  AddDeveloperCard(Page, 'Serdar Kayışkıran', 'avatar-sk.bmp', 'serdarkayiskiran', 'https://www.linkedin.com/in/serdarkayiskiran/', CardWidth + Gap, Top, CardWidth, CardHeight);
  AddButton(Page.Surface, CustomMessage('DevelopersProject'), '{#RepositoryUrl}', 0, Top + CardHeight + ScaleY(18), ScaleX(150));
end;

procedure InitializeWizard();
begin
  WizardForm.WelcomeLabel2.Caption := WizardForm.WelcomeLabel2.Caption + #13#10#13#10 + CustomMessage('DevelopedBy');
  CreateDevelopersPage();
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = wpFinished then
    WizardForm.FinishedLabel.Caption := WizardForm.FinishedLabel.Caption + #13#10#13#10 + CustomMessage('DevelopedBy');
end;

function NextVersionPart(var Version: String): Integer;
var Dot: Integer; Part: String;
begin
  Dot := Pos('.', Version);
  if Dot = 0 then begin Part := Version; Version := ''; end
  else begin Part := Copy(Version, 1, Dot - 1); Delete(Version, 1, Dot); end;
  Result := StrToIntDef(Part, 0);
end;

function VersionAtLeast(Current, Required: String): Boolean;
var I, A, B: Integer;
begin
  if Copy(Current, 1, 1) = 'v' then Delete(Current, 1, 1);
  Result := True;
  for I := 1 to 4 do begin
    A := NextVersionPart(Current); B := NextVersionPart(Required);
    if A > B then Exit;
    if A < B then begin Result := False; Exit; end;
  end;
end;

function VcInstalledAt(Root: Integer): Boolean;
var Version: String; Installed: Cardinal;
begin
  Result := RegQueryDWordValue(Root, 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64', 'Installed', Installed)
    and (Installed = 1)
    and RegQueryStringValue(Root, 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64', 'Version', Version);
  if Result then Result := VersionAtLeast(Version, '{#VcVersion}');
end;

function ToolInstalled(const Path, Minimum: String): Boolean;
var Version: String;
begin
  Result := GetVersionNumbersString(Path, Version);
  if Result then Result := VersionAtLeast(Version, Minimum);
end;

function RunPrerequisite(const ProgramName, Arguments, DisplayName: String): String;
var ResultCode: Integer;
begin
  Log('Installing prerequisite: ' + DisplayName);
  Result := '';
  if not Exec(ProgramName, Arguments, '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
    Result := 'Could not start ' + DisplayName + ': ' + SysErrorMessage(ResultCode)
  else if (ResultCode = 3010) or (ResultCode = 1641) then
    PrerequisiteRestart := True
  else if ResultCode <> 0 then
    Result := DisplayName + ' installation failed (exit ' + IntToStr(ResultCode) + '). Please check the setup log.';
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not (VcInstalledAt(HKLM64) or VcInstalledAt(HKLM32)) then begin
    ExtractTemporaryFile('vc_redist.x64.exe');
    Result := RunPrerequisite(ExpandConstant('{tmp}\vc_redist.x64.exe'), '/install /quiet /norestart', 'Microsoft Visual C++ Runtime');
    if Result <> '' then Exit;
    if not (VcInstalledAt(HKLM64) or VcInstalledAt(HKLM32)) then begin
      Result := 'Microsoft Visual C++ Runtime installation could not be verified.'; Exit;
    end;
  end;
  if WizardIsComponentSelected('usb') then begin
    if not ToolInstalled(ExpandConstant('{commonpf64}\usbipd-win\usbipd.exe'), '5.3.0.0') then begin
      ExtractTemporaryFile('usbipd-win_x64.msi');
      Result := RunPrerequisite(ExpandConstant('{sys}\msiexec.exe'), '/i "' + ExpandConstant('{tmp}\usbipd-win_x64.msi') + '" /qn /norestart', 'usbipd-win');
      if Result <> '' then Exit;
      if not ToolInstalled(ExpandConstant('{commonpf64}\usbipd-win\usbipd.exe'), '5.3.0.0') then begin
        Result := 'usbipd-win installation could not be verified.'; Exit;
      end;
    end;
    if not ToolInstalled(ExpandConstant('{commonpf64}\USBip\usbip.exe'), '0.9.8.1') then begin
      ExtractTemporaryFile('usbip-win2_x64.exe');
      Result := RunPrerequisite(ExpandConstant('{tmp}\usbip-win2_x64.exe'), '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /COMPONENTS="main,client" /TASKS=""', 'USBip client drivers');
      if Result <> '' then Exit;
      if not ToolInstalled(ExpandConstant('{commonpf64}\USBip\usbip.exe'), '0.9.8.1') then begin
        Result := 'USBip client installation could not be verified.'; Exit;
      end;
      PrerequisiteRestart := True;
    end;
  end;
end;

function NeedRestart(): Boolean;
begin
  Result := PrerequisiteRestart;
end;

function CanLaunch(): Boolean;
begin
  Result := not PrerequisiteRestart;
end;

function LaunchAfterUpdate(): Boolean;
begin
  Result := (ExpandConstant('{param:MAUSEUPDATE|0}') = '1') and CanLaunch();
end;
