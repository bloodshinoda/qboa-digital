@echo off
setlocal EnableExtensions EnableDelayedExpansion
chcp 65001 >nul
title QBoa Digital - Manutenção do Tela Azul
mode con: cols=112 lines=48
color 1F

rem ============================================================
rem QBOA DIGITAL 2.0
rem Manutenção automatizada do Tela Azul
rem Execute como Administrador
rem ============================================================

set "ROOT=%~dp0"
set "BIN=%ROOT%bin"
set "LOGDIR=%ROOT%logs"
set "BLEACH=%BIN%\bleachbit_console.exe"
set "VERSION=2.0"

if not exist "%LOGDIR%" md "%LOGDIR%" >nul 2>&1

call :GetTimestamp
set "LOGFILE=%LOGDIR%\QBoaDigital_%STAMP%.log"
call :Log "QBoa Digital %VERSION% iniciado."

rem ---------- Elevacao ----------
fltmc >nul 2>&1
if not "%errorlevel%"=="0" (
    echo.
    echo  Este programa precisa ser executado como PATRÃO.
    echo  Solicitando promoção...
    powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

:MENU
cls
call :Logo
echo.
echo  ==========================================================================================================
echo.
echo      [1] QBoa EXPRESS                         Varrer onde o papa passa
echo      [2] QBoa EXPRESS + DESLIGAR              Varrer onde o papa passa e vazar
echo.
echo      [3] QBoa COMPLETA                        Passar o rodo com vontade
echo      [4] QBoa COMPLETA + DESLIGAR             Passar o rodo com vontade e vazar
echo.
echo      [5] QBoa TURBO                           Meter QBoa, Sapólio e a porra toda
echo      [6] QBoa TURBO + DESLIGAR                Meter QBoa, Sapólio e a porra toda, e vazar
echo.
echo  ----------------------------------------------------------------------------------------------------------
echo      [7] Perguntar para os universitários
echo      [8] Sobre o QBoa Digital
echo      [0] Sair
echo.
echo  ==========================================================================================================
set "choice="
set /p "choice=  Escolha uma opção e deixe a QBoa agir: "

if "%choice%"=="1" call :RunMode LEVE NAO & goto MENU
if "%choice%"=="2" call :RunMode LEVE SIM & exit /b
if "%choice%"=="3" call :RunMode MEDIA NAO & goto MENU
if "%choice%"=="4" call :RunMode MEDIA SIM & exit /b
if "%choice%"=="5" call :RunMode PESADA NAO & goto MENU
if "%choice%"=="6" call :RunMode PESADA SIM & exit /b
if "%choice%"=="7" call :Diagnostico & goto MENU
if "%choice%"=="8" call :Sobre & goto MENU
if "%choice%"=="0" exit /b

echo.
echo  Escolha inexistente.
timeout /t 2 >nul
goto MENU


:RunMode
set "MODE=%~1"
set "SHUT=%~2"
cls
call :Logo
echo.
echo  MODO SELECIONADO: %MODE%
echo  Inicio: %date% %time%
call :Log "Modo selecionado: %MODE%"

call :Step 1 "Verificando e reduzindo xeretagem do tio Bill"
call :DisableTelemetry

if /i "%MODE%"=="LEVE" (
    call :Step 2 "Varrendo só onde o papa passa"
    call :LightCleanup
    call :Step 3 "QBoazinha de leve"
    call :BleachLight
)

if /i "%MODE%"=="MEDIA" (
    call :Step 2 "Limpando com um pouco de atenção"
    call :MediumCleanup
    call :Step 3 "Passando uma QBoa com vontade"
    call :BleachMedium
    call :Step 4 "Verificando se tu pagou o dízimo"
    call :RunDISMScan
)

if /i "%MODE%"=="PESADA" (
    call :Step 2 "Chamando testemunhas se der merda"
    call :RestorePoint
    call :Step 3 "Recolhendo a lixarada com vontade"
    call :HeavyCleanup
    call :Step 4 "Enxarcando tudo de QBoa com muita vontade"
    call :BleachFull
    call :Step 5 "Mandando o Bill Gates se foder"
    call :RunDISMCleanup
    call :Step 6 "Pagando o dízimo"
    call :RunDISMRestore
    call :Step 7 "Olhando se tu tá no SPC"
    call :RunSFC
    call :Step 8 "Botando kit padaria no teu fusca"
    call :RunWinUtilConservative
)

call :Finish "%MODE%" "%SHUT%"
exit /b


:Step
echo.
echo  ------------------------------------------------------------------------------------------
echo  [%~1] %~2
echo  ------------------------------------------------------------------------------------------
call :Log "[%~1] %~2"
exit /b


:DisableTelemetry
rem Servicos frequentemente associados a diagnostico/telemetria.
rem Alguns podem nao existir em todas as versoes do Windows.
for %%S in (DiagTrack dmwappushservice) do (
    sc query "%%S" >nul 2>&1
    if not errorlevel 1 (
        sc stop "%%S" >nul 2>&1
        sc config "%%S" start= disabled >nul 2>&1
        call :Log "Servico %%S processado."
    )
)

rem Tarefas de diagnostico selecionadas. Nao remove arquivos nem componentes.
for %%T in (
"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser"
"\Microsoft\Windows\Application Experience\ProgramDataUpdater"
"\Microsoft\Windows\Customer Experience Improvement Program\Consolidator"
"\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip"
"\Microsoft\Windows\Customer Experience Improvement Program\KernelCeipTask"
"\Microsoft\Windows\Autochk\Proxy"
) do (
    schtasks /Change /TN %%T /Disable >nul 2>&1
)

rem Politicas de coleta de diagnostico.
reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\DataCollection" /v AllowTelemetry /t REG_DWORD /d 0 /f >nul 2>&1
reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\DataCollection" /v DisableEnterpriseAuthProxy /t REG_DWORD /d 1 /f >nul 2>&1

echo  [OK] Telemetria e tarefas selecionadas foram verificadas.
call :Log "Telemetria processada."
exit /b


:LightCleanup
del /f /s /q "%TEMP%\*" >nul 2>&1
for /d %%D in ("%TEMP%\*") do rd /s /q "%%D" >nul 2>&1

del /f /s /q "%WINDIR%\Temp\*" >nul 2>&1
for /d %%D in ("%WINDIR%\Temp\*") do rd /s /q "%%D" >nul 2>&1

powershell -NoProfile -Command "Clear-RecycleBin -Force -ErrorAction SilentlyContinue" >nul 2>&1
call :Log "Limpeza leve concluída."
echo  [OK] Temporários e Lixeira processados.
exit /b


:MediumCleanup
call :LightCleanup

rem Caches de navegador comuns - somente arquivos temporarios/cache.
for %%P in (
"%LOCALAPPDATA%\Google\Chrome\User Data\Default\Cache"
"%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Cache"
"%LOCALAPPDATA%\Mozilla\Firefox\Profiles"
) do (
    if exist %%P (
        rem Firefox e tratado preferencialmente pelo BleachBit.
    )
)

rem Limpeza de arquivos de entrega, sem apagar dados do usuario.
del /f /s /q "%WINDIR%\SoftwareDistribution\Download\*.tmp" >nul 2>&1
call :Log "Limpeza intermediária concluída."
exit /b


:HeavyCleanup
call :MediumCleanup

rem Limpeza adicional segura de dumps temporarios antigos.
del /f /q "%WINDIR%\Minidump\*.tmp" >nul 2>&1
del /f /q "%WINDIR%\LiveKernelReports\*.tmp" >nul 2>&1

rem Disk Cleanup somente se o perfil /sageset:1 ja tiver sido configurado pelo usuario.
if exist "%SystemRoot%\System32\cleanmgr.exe" (
    echo  [INFO] Executando Disk Cleanup com o perfil ja configurado (sagerun:1).
    cleanmgr /sagerun:1
)

call :Log "Limpeza pesada local concluída."
exit /b


:BleachLight
if not exist "%BLEACH%" (
    echo  [AVISO] Binário da QBoa não encontrado em "%BIN%".
    call :Log "QBoa bin não encontrado."
    exit /b
)

"%BLEACH%" --clean system.recycle_bin system.tmp windows_defender.temp
call :Log "Qboa leve executada. Código: %errorlevel%"
exit /b


:BleachMedium
if not exist "%BLEACH%" (
    echo  [AVISO] Binário da Qboa nao encontrado em "%BIN%".
    call :Log "QBoa bin não encontrado."
    exit /b
)

"%BLEACH%" --clean ^
deepscan.ds_store deepscan.tmp ^
firefox.cache firefox.crash_reports firefox.session firefox.vacuum ^
google_chrome.cache google_chrome.session google_chrome.crash_reports google_chrome.vacuum ^
internet_explorer.cache internet_explorer.logs ^
microsoft_edge.cache microsoft_edge.crash_reports microsoft_edge.session microsoft_edge.vacuum ^
microsoft_office.debug_logs microsoft_office.mru ^
system.clipboard system.recycle_bin system.tmp ^
thunderbird.cache thunderbird.sessionjson thunderbird.vacuum ^
vlc.memory_dump vlc.mru ^
windows_defender.temp winrar.history winrar.temp

call :Log "QBoa intermediária executada. Codigo: %errorlevel%"
exit /b


:BleachFull
if not exist "%BLEACH%" (
    echo  [AVISO] Binário da QBoa não encontrado em "%BIN%".
    call :Log "Qboa bin não encontrado."
    exit /b
)

rem MANTIDO: perfil completo baseado na sua lista original.
"%BLEACH%" --clean ^
deepscan.ds_store deepscan.tmp ^
firefox.cache firefox.crash_reports firefox.session firefox.site_data firefox.vacuum ^
flash.cache flash.cookies ^
google_chrome.cache google_chrome.session google_chrome.crash_reports google_chrome.vacuum google_chrome.site_data ^
internet_explorer.cache internet_explorer.logs ^
libreoffice.history ^
microsoft_edge.cache microsoft_edge.crash_reports microsoft_edge.session microsoft_edge.site_data microsoft_edge.vacuum ^
microsoft_office.debug_logs microsoft_office.mru ^
system.clipboard system.recycle_bin system.tmp ^
thunderbird.cache thunderbird.sessionjson thunderbird.vacuum ^
vlc.memory_dump vlc.mru ^
windows_defender.temp ^
winrar.history winrar.temp

call :Log "QBoa completa executada. Codigo: %errorlevel%"
exit /b


:RunDISMScan
DISM /Online /Cleanup-Image /ScanHealth
call :Log "Verificação de dízimo concluída. Codigo: %errorlevel%"
exit /b


:RunDISMCleanup
DISM /Online /Cleanup-Image /StartComponentCleanup
call :Log "Pagamento do dízimo concluído. Codigo: %errorlevel%"
exit /b


:RunDISMRestore
DISM /Online /Cleanup-Image /RestoreHealth
call :Log "Oferta no altar pro pastor ladrão efetuada. Codigo: %errorlevel%"
exit /b


:RunSFC
sfc /scannow
call :Log "Verificação no SPC concluída. Codigo: %errorlevel%"
exit /b


:RestorePoint
powershell -NoProfile -ExecutionPolicy Bypass -Command "try { Checkpoint-Computer -Description 'QBoa Digital - Antes da limpeza pesada' -RestorePointType 'MODIFY_SETTINGS'; exit 0 } catch { exit 1 }" >nul 2>&1
if errorlevel 1 (
    echo  [INFO] Não foi possível criar ponto de restauração neste momento.
    call :Log "Ponto de restauração não criado."
) else (
    echo  [OK] Ponto de restauração criado.
    call :Log "Ponto de restauração criado."
)
exit /b


:RunWinUtilConservative
rem Perfil conservador: WinUtil Minimal.
rem A chamada abaixo baixa a versao estavel atual e executa somente o preset.
rem O WinUtil e voltado atualmente ao Windows 11; em Windows 10 esta etapa e ignorada.
for /f "tokens=4-5 delims=. " %%a in ('ver') do set "WINVER=%%a.%%b"
echo %WINVER% | find "10." >nul
if not errorlevel 1 (
    echo  [INFO] WinUtil atual nao oferece suporte oficial ao Windows 10. Etapa ignorada.
    call :Log "WinUtil ignorado: Windows 10."
    exit /b
)

powershell -NoProfile -ExecutionPolicy Bypass -Command "& ([ScriptBlock]::Create((irm https://christitus.com/win))) -Preset Minimal"
call :Log "WinUtil Minimal executado. Codigo: %errorlevel%"
exit /b


:Finish
set "FINMODE=%~1"
set "FINSHUT=%~2"
call :Log "Modo %FINMODE% finalizado."

echo.
echo  ==========================================================================================================
echo.
echo      LIMPEZA CONCLUIDA COM SUCESSO!
echo.
echo      QBoa Digital terminou o trabalho. Seu computador pode descansar um pouco.
echo.
echo  ==========================================================================================================

if /i "%FINSHUT%"=="NAO" (
    call :DonationQR
    echo.
    echo  Pressione qualquer tecla para voltar ao menu...
    pause >nul
) else (
    echo.
    echo  O computador será esbofeteado em 15 segundos.
    echo  Para cancelar, pressione CTRL+C nesta janela.
    shutdown /s /t 15 /c "QBoa Digital: manutencao concluída, pode chamar as puta."
    timeout /t 15 >nul
)
exit /b


:DonationQR
echo.
echo  ----------------------------------------------------------------------------------------------------------
echo.
echo    SE O QBOA DIGITAL LHE AJUDOU, UMA CONTRIBUIÇÃO AJUDA O PROJETO A CONTINUAR MELHORANDO,
echo    E EU A ENCHER A CARA. ESCANEIE O QR CODE ABAIXO COM O APLICATIVO DO SEU BANCO:
echo.
echo.                                           
echo    █▀▀▀▀▀█ ▀▀██▄▀▀▄▀▀▄▀█ ▀▀█▀ ▀  █▀▀▀▀▀█  
echo    █ ███ █ ▀▄ ▄ ▄▄▄█ ▀▀█▄▀▀▀█ ▀█ █ ███ █  
echo    █ ▀▀▀ █ ▄█▀██▀▄█▄▄██ ▀█▀▀▄ ▀▄ █ ▀▀▀ █  
echo    ▀▀▀▀▀▀▀ ▀ ▀ █ ▀▄█▄▀▄█▄█▄▀ ▀▄▀ ▀▀▀▀▀▀▀  
echo    █▄█ ▄ ▀▀▄▀ ▄▄ ▄  █  █▀  ▀▄▀▄▄  ▀  █ ▀  
echo    ▀▀███▄▀ ▄ ▄▀▀▀▄▀ ▀▀▄██▄█▀ ▄  ▀▀▀██▀▀▀  
echo    ▄█▄▄▄ ▀▀ ▄█▄▀▀▄▀█▀ ▄▄▄▄▀▄ ▄▀▄  ▀▀▄▀█▀  
echo    ▄███▄▄▀█▀▄  ▄  ▄▄▀█ ▄ ▀█  ▄▄▄██▀▀▀▀█   
echo    ▀▀█▀▄█▀ █▄█▀ ▄█▀█▀█▀ ▄▄█▀▄ ▄▀▀▄  ▀█ ▀  
echo    ▄▀▄▄▀ ▀ ▀█  █ ▄▄█▀ ▀  ▀█▀ █ ▀▄▀█▀▄█▀▀  
echo    ▀▄▀▀▄▀▀▄▀▀▄█▀ █  ▄██▀████  █▀▀▀▀▀ ▀▀▀  
echo     ████ ▀ █▀ ▄▄▀█▀▀▄ ▄▀ ▀▀ ▄█ ▀▀█▀▄█▀▀   
echo    ▀ ▄▀▄▀▀▀█▄▀▄▀█▄  ▀ ▀█▀▀█▀█▀▄▀▄ █▄▀▀▀   
echo    ▀▀█▀▄█▀▄ ▀  █ ▀▀ ▀▀▄█▀▀▀▀▀█▄█▄▀▀▄▄ ▀█  
echo    ▀▀▀ ▀▀▀▀▄▄█ █ ██▀▀  ▀█ ▀▀  ▀█▀▀▀█▀▀▄   
echo    █▀▀▀▀▀█ ▀█  ▄▀▀▄ ▀█▀▀▄ ▀ ▀  █ ▀ █  ▄   
echo    █ ███ █  █▄█  ▀▀█▀█▀ ▀ █▄▀▄███▀▀▀█▀▀   
echo    █ ▀▀▀ █ ▀▀  ▀▀█▄█▀█▀  ██▄ ▀▄█  ▀   ▀█  
echo    ▀▀▀▀▀▀▀ ▀▀   ▀  ▀▀ ▀▀ ▀▀ ▀▀▀   ▀▀  ▀▀  
echo.                                           
echo.
echo    Obrigado por apoiar o desenvolvimento do QBoa Digital!
echo    A chave PIX não é exibida em texto nesta janela.
echo.
echo  ----------------------------------------------------------------------------------------------------------
exit /b


:Diagnostico
cls
call :Logo
echo.
echo  DIAGNÓSTICO DO SISTEMA
echo  ----------------------------------------------------------------------------------------------------------
echo.
systeminfo | findstr /B /C:"OS Name" /C:"OS Version" /C:"System Type"
echo.
echo  Espaço livre:
wmic logicaldisk where "DeviceID='C:'" get DeviceID^,FreeSpace^,Size /value 2>nul
echo.
echo  Status do serviço DiagTrack:
sc query DiagTrack 2>nul | findstr /I "STATE"
echo.
echo  Arquivo de log atual:
echo  %LOGFILE%
echo.
pause
exit /b


:Sobre
cls
call :Logo
echo.
echo  QBOA DIGITAL %VERSION%
echo  by Vilson de Oliveira Junior
echo.
echo  Utilitário de manutenção automatizada para Windows.
echo  Filosofia: escolha uma opção, deixe a QBoa agir e volte depois.
echo.
echo  EXPRESS   - tarefas realmente leves.
echo  COMPLETA  - limpeza ampliada e verificação moderada.
echo  TURBO     - QBoa completa, Dízimo e SPC.
echo.
echo  Também fazemos com que seu Windows pare de ligar pra casa.
echo.
echo  Logs: %LOGDIR%
echo.
pause
exit /b


:Logo
color 1F
echo.
echo                    bbbbbbbb
echo       QQQQQQQQQ    b::::::b
echo     QQ:::::::::QQ  b::::::b
echo   QQ:::::::::::::QQb::::::b
echo  Q:::::::QQQ:::::::Qb:::::b
echo  Q::::::O   Q::::::Qb:::::bbbbbbbbb       ooooooooooo     aaaaaaaaaaaaa
echo  Q:::::O     Q:::::Qb::::::::::::::bb   oo:::::::::::oo   a::::::::::::a
echo  Q:::::O     Q:::::Qb::::::::::::::::b o:::::::::::::::o  aaaaaaaaa:::::a
echo  Q:::::O     Q:::::Qb:::::bbbbb:::::::bo:::::ooooo:::::o           a::::a
echo  Q:::::O     Q:::::Qb:::::b    b::::::bo::::o     o::::o    aaaaaaa::::a
echo  Q:::::O     Q:::::Qb:::::b     b:::::bo::::o     o::::o  aa::::::::::::a
echo  Q:::::O  QQQQ:::::Qb:::::b     b:::::bo::::o     o::::o a::::aaaa::::::a
echo  Q::::::O Q::::::::Qb:::::b     b:::::bo::::o     o::::oa::::a    a:::::a
echo  Q:::::::QQ::::::::Qb:::::bbbbbb::::::bo:::::ooooo:::::oa::::a    a:::::a
echo   QQ::::::::::::::Q b::::::::::::::::b o:::::::::::::::oa:::::aaaa::::::a
echo     QQ:::::::::::Q  b:::::::::::::::b   oo:::::::::::oo  a::::::::::aa:::a
echo       QQQQQQQQ::::QQbbbbbbbbbbbbbbbb      ooooooooooo     aaaaaaaaaa  aaaa
echo               Q:::::Q
echo                QQQQQQ
echo.
echo  D           I           G           I           T           A           L
exit /b


:GetTimestamp
for /f %%I in ('powershell -NoProfile -Command "Get-Date -Format yyyy-MM-dd_HH-mm-ss"') do set "STAMP=%%I"
exit /b


:Log
>>"%LOGFILE%" echo [%date% %time%] %~1
exit /b
