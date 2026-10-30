<#
.SYNOPSIS
  AI Cluster - testa a API do gateway a partir de um Windows: mede a velocidade e conversa com o modelo.

.DESCRIPTION
  Confere /health e /v1/models, e então:
    - sem -Chat: faz uma pergunta (ou -Runs vezes) e mostra, por resposta, o tempo até a primeira
      palavra (TTFT), o tempo total e os tokens por segundo, com um resumo no fim;
    - com -Chat: abre uma conversa em que você e o modelo se alternam, com histórico, e cada resposta
      vem com as mesmas medidas.
  Sem -Model usa o primeiro modelo de linguagem da lista. A chave está em /etc/ai-cluster/gateway.env,
  no servidor (AI_CLUSTER_API_KEY), ou é a que o `ai-cluster model start` mostrou.
  Os tokens são contados pelos pedaços que o gateway envia (cerca de um por token).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File .\aic-test-api.ps1 -Url http://192.168.1.25:8080 -Key SUA_CHAVE

.EXAMPLE
  .\aic-test-api.ps1 -Url http://192.168.1.25:8080 -Key SUA_CHAVE -Runs 5 -Prompt "Conte até 10"

.EXAMPLE
  .\aic-test-api.ps1 -Url http://192.168.1.25:8080 -Key SUA_CHAVE -Chat
#>
param(
  [string]$Url = "",
  [string]$Key = $env:AIC_API_KEY,
  [string]$Model = "",
  [string]$Prompt = "",
  [int]$MaxTokens = 200,
  [int]$Runs = 1,
  [switch]$Chat,
  [string]$System = "",
  [ValidateSet("pt", "en")][string]$Lang = "pt"
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Net.Http

function T([string]$pt, [string]$en) { if ($Lang -eq "en") { $en } else { $pt } }
function Banner([string]$title) {
  Write-Host ""
  Write-Host "  AI Cluster  " -NoNewline -ForegroundColor Cyan
  Write-Host $title -ForegroundColor DarkGray
  Write-Host ("  " + ("-" * 56)) -ForegroundColor DarkGray
  Write-Host ""
}
function Ok([string]$text) { Write-Host "  [ok] " -NoNewline -ForegroundColor Green; Write-Host $text }
function Fail([string]$text) {
  Write-Host ""
  Write-Host "  [x] $text" -ForegroundColor Red
  Write-Host ""
  Wait-Close
  exit 1
}
function Sec([double]$s) { ([math]::Round($s, 2)).ToString([Globalization.CultureInfo]::InvariantCulture) + "s" }
function Rate([double]$r) { ([math]::Round($r, 1)).ToString([Globalization.CultureInfo]::InvariantCulture) }

# Aberto com duplo clique (sem parâmetros), o script pergunta o que falta e não fecha sozinho.
$guided = [string]::IsNullOrWhiteSpace($Url) -and [string]::IsNullOrWhiteSpace($Key)
function Wait-Close { if ($guided) { Write-Host ""; Read-Host (T "  Enter para fechar" "  Press Enter to close") | Out-Null } }
if ($guided) {
  Banner (T "Teste da API" "API test")
  Write-Host "  $(T 'Responda o que falta. Enter aceita o valor entre colchetes.' 'Answer what is missing. Enter accepts the value in brackets.')" -ForegroundColor DarkGray
  Write-Host ""
}
if (-not $Url) {
  $default = "http://192.168.1.25:8080"
  $typed = Read-Host (T "  Endereço do gateway [$default]" "  Gateway address [$default]")
  $Url = if ($typed) { $typed.Trim() } else { $default }
}
# Aceita "192.168.1.25", "192.168.1.25:8080" ou o endereço completo.
if ($Url -notmatch "^https?://") { $Url = "http://$Url" }
if ($Url -match "^https?://[^/:]+$") { $Url = "${Url}:8080" }
if (-not $Key) {
  $Key = (Read-Host (T "  Chave de API (no servidor: /etc/ai-cluster/gateway.env, linha AI_CLUSTER_API_KEY)" "  API key (on the server: /etc/ai-cluster/gateway.env, line AI_CLUSTER_API_KEY)")).Trim()
  if ($Key -match "^AI_CLUSTER_API_KEY=") { $Key = $Key.Substring("AI_CLUSTER_API_KEY=".Length) }
}
if ($guided -and -not $Chat) {
  $mode = Read-Host (T "  [M]edir a velocidade ou [C]onversar? [M]" "  [M]easure speed or [C]hat? [M]")
  if ($mode -match "^[cC]") { $Chat = [switch]$true }
}
if (-not $Key) {
  Fail (T "informe -Key (ou a variável AIC_API_KEY): está em /etc/ai-cluster/gateway.env no servidor" "give -Key (or the AIC_API_KEY variable): it is in /etc/ai-cluster/gateway.env on the server")
}
$Url = $Url.TrimEnd("/")
if (-not $Prompt) { $Prompt = T "Responda em uma frase: o que é um cluster?" "Answer in one sentence: what is a cluster?" }

$http = New-Object System.Net.Http.HttpClient
$http.Timeout = [TimeSpan]::FromSeconds(600)
$http.DefaultRequestHeaders.Authorization = New-Object System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", $Key)

# Devolve @{ Code; Body } sem lançar exceção por código HTTP de erro.
function Call([string]$method, [string]$path) {
  $req = New-Object System.Net.Http.HttpRequestMessage([System.Net.Http.HttpMethod]::$method, "$Url$path")
  $res = $http.SendAsync($req).GetAwaiter().GetResult()
  @{ Code = [int]$res.StatusCode; Body = $res.Content.ReadAsStringAsync().GetAwaiter().GetResult() }
}

# Manda a conversa e lê a resposta aos pedaços, medindo o tempo até o primeiro e o total.
# Devolve @{ Ok; Error; Text; Ttft; Total; Chunks; DecodeRate; OverallRate }.
function Ask($messages, [bool]$live) {
  $json = @{ model = $Model; messages = $messages; max_tokens = $MaxTokens; stream = $true } | ConvertTo-Json -Depth 6 -Compress
  $req = New-Object System.Net.Http.HttpRequestMessage([System.Net.Http.HttpMethod]::Post, "$Url/v1/chat/completions")
  $req.Content = New-Object System.Net.Http.StringContent($json, [Text.Encoding]::UTF8, "application/json")
  $clock = [Diagnostics.Stopwatch]::StartNew()
  try { $res = $http.SendAsync($req, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult() }
  catch { return @{ Ok = $false; Error = $_.Exception.InnerException.Message } }
  if ([int]$res.StatusCode -ne 200) {
    $raw = $res.Content.ReadAsStringAsync().GetAwaiter().GetResult()
    $detail = try { ($raw | ConvertFrom-Json).error.message } catch { $raw }
    return @{ Ok = $false; Error = "HTTP $([int]$res.StatusCode): $detail" }
  }
  $reader = New-Object System.IO.StreamReader($res.Content.ReadAsStreamAsync().GetAwaiter().GetResult(), [Text.Encoding]::UTF8)
  $text = New-Object System.Text.StringBuilder
  $chunks = 0; $ttft = $null
  while (($line = $reader.ReadLine()) -ne $null) {
    if (-not $line.StartsWith("data:")) { continue }
    $data = $line.Substring(5).Trim()
    if ($data -eq "[DONE]") { break }
    try { $piece = ($data | ConvertFrom-Json).choices[0].delta.content } catch { continue }
    if (-not $piece) { continue }
    if ($null -eq $ttft) { $ttft = $clock.Elapsed.TotalSeconds }
    $chunks++
    [void]$text.Append($piece)
    if ($live) { Write-Host $piece -NoNewline }
  }
  $total = $clock.Elapsed.TotalSeconds
  if ($null -eq $ttft) { $ttft = $total }
  $decode = if ($chunks -gt 1 -and $total -gt $ttft) { ($chunks - 1) / ($total - $ttft) } else { 0 }
  $overall = if ($total -gt 0) { $chunks / $total } else { 0 }
  @{ Ok = $true; Text = $text.ToString(); Ttft = $ttft; Total = $total; Chunks = $chunks; DecodeRate = $decode; OverallRate = $overall }
}

function Stats($r) {
  (T "primeira palavra" "first token") + " " + (Sec $r.Ttft) + "  |  " + (T "total" "total") + " " + (Sec $r.Total) +
    "  |  ~$($r.Chunks) tokens  |  " + (Rate $r.DecodeRate) + " " + (T "tokens/s gerando" "tokens/s generating") +
    " (" + (Rate $r.OverallRate) + " " + (T "no total" "overall") + ")"
}

Banner (T "Teste da API" "API test")
Write-Host "  $Url" -ForegroundColor DarkGray
Write-Host ""

# 1) o gateway está de pé?
try { $h = Call "Get" "/health" } catch {
  Fail (T "o gateway não respondeu em $Url/health: confira o endereço, a porta e o firewall do Windows ($($_.Exception.InnerException.Message))" "the gateway did not answer at $Url/health: check the address, port and Windows firewall ($($_.Exception.InnerException.Message))")
}
if ($h.Code -ne 200) { Fail (T "/health respondeu $($h.Code)" "/health answered $($h.Code)") }
Ok (T "gateway no ar (/health)" "gateway is up (/health)")

# 2) a chave vale e há modelo de linguagem?
$m = Call "Get" "/v1/models"
if ($m.Code -eq 401 -or $m.Code -eq 403) { Fail (T "a chave de API foi recusada" "the API key was refused") }
if ($m.Code -ne 200) { Fail (T "/v1/models respondeu $($m.Code)" "/v1/models answered $($m.Code)") }
$language = @(($m.Body | ConvertFrom-Json).data | Where-Object { $_.kind -eq "language-model" })
if (-not $Model) { if ($language.Count -gt 0) { $Model = $language[0].id } }
if (-not $Model) {
  Fail (T "nenhum modelo de linguagem ativo. No servidor: ai-cluster model start <id>" "no language model is active. On the server: ai-cluster model start <id>")
}
Ok (T "modelo: $Model" "model: $Model")

# 3a) conversa: você e o modelo se alternam, com histórico
if ($Chat) {
  $history = New-Object System.Collections.ArrayList
  if ($System) { [void]$history.Add(@{ role = "system"; content = $System }) }
  $turns = @()
  Write-Host ""
  Write-Host "  $(T 'Conversa aberta. Comandos: /sair  /limpar  /medidas' 'Chat open. Commands: /exit  /clear  /stats')" -ForegroundColor DarkGray
  while ($true) {
    Write-Host ""
    $you = Read-Host (T "  você" "  you")
    if ($null -eq $you) { break }
    $you = $you.Trim()
    if (-not $you) { continue }
    if ($you -in @("/sair", "/exit", "/quit")) { break }
    if ($you -in @("/limpar", "/clear")) {
      $history.Clear(); if ($System) { [void]$history.Add(@{ role = "system"; content = $System }) }
      Ok (T "histórico limpo" "history cleared"); continue
    }
    if ($you -in @("/medidas", "/stats")) {
      if ($turns.Count -eq 0) { Write-Host "  $(T 'ainda sem respostas' 'no answers yet')"; continue }
      $avgT = ($turns | ForEach-Object { $_.Ttft } | Measure-Object -Average).Average
      $avgR = ($turns | ForEach-Object { $_.DecodeRate } | Measure-Object -Average).Average
      Write-Host "  $($turns.Count) $(T 'respostas' 'answers'): $(T 'primeira palavra média' 'mean first token') $(Sec $avgT), $(T 'velocidade média' 'mean speed') $(Rate $avgR) tokens/s"
      continue
    }
    [void]$history.Add(@{ role = "user"; content = $you })
    Write-Host ""
    Write-Host "  $(T 'modelo' 'model')> " -NoNewline -ForegroundColor Cyan
    $r = Ask @($history) $true
    Write-Host ""
    if (-not $r.Ok) {
      Write-Host "  [x] $($r.Error)" -ForegroundColor Red
      [void]$history.RemoveAt($history.Count - 1)
      continue
    }
    [void]$history.Add(@{ role = "assistant"; content = $r.Text })
    $turns += , $r
    Write-Host "  $(Stats $r)" -ForegroundColor DarkGray
  }
  Write-Host ""
  Wait-Close
  exit 0
}

# 3b) medição: uma ou várias perguntas iguais
Write-Host ""
Write-Host "  $(T 'Pergunta' 'Question')" -ForegroundColor White
Write-Host "  $Prompt"
$msgs = @()
if ($System) { $msgs += , @{ role = "system"; content = $System } }
$msgs += , @{ role = "user"; content = $Prompt }
$all = @()
for ($i = 1; $i -le $Runs; $i++) {
  Write-Host ""
  if ($Runs -gt 1) { Write-Host "  $(T 'Rodada' 'Run') $i/$Runs" -ForegroundColor White } else { Write-Host "  $(T 'Resposta' 'Answer')" -ForegroundColor White }
  Write-Host "  " -NoNewline
  $r = Ask $msgs ($Runs -eq 1)
  if (-not $r.Ok) { Fail (T "o chat falhou: $($r.Error)" "chat failed: $($r.Error)") }
  if ($Runs -gt 1) { $flat = $r.Text -replace "\s+", " "; Write-Host ($flat.Substring(0, [math]::Min(70, $flat.Length)) + "...") }
  Write-Host ""
  Ok (Stats $r)
  $all += , $r
}
if ($Runs -gt 1) {
  $ttfts = $all | ForEach-Object { $_.Ttft } | Sort-Object
  $rates = $all | ForEach-Object { $_.DecodeRate }
  Write-Host ""
  Write-Host "  $(T 'Resumo' 'Summary') ($Runs $(T 'rodadas' 'runs'))" -ForegroundColor White
  Write-Host "  $(T 'primeira palavra' 'first token'): $(T 'média' 'mean') $(Sec (($ttfts | Measure-Object -Average).Average)), $(T 'mínima' 'min') $(Sec $ttfts[0]), $(T 'máxima' 'max') $(Sec $ttfts[-1])"
  Write-Host "  $(T 'velocidade' 'speed'): $(T 'média' 'mean') $(Rate (($rates | Measure-Object -Average).Average)) tokens/s, $(T 'mínima' 'min') $(Rate (($rates | Measure-Object -Minimum).Minimum)), $(T 'máxima' 'max') $(Rate (($rates | Measure-Object -Maximum).Maximum))"
}
Write-Host ""
Wait-Close
