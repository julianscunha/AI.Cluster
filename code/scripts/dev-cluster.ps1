# Cluster de desenvolvimento em contêineres, para acompanhar com a CLI (versão PowerShell).
#
#   .\code\scripts\dev-cluster.ps1 build        compila os binários no volume aic-target
#   .\code\scripts\dev-cluster.ps1 up [N]       sobe o controller e N workers (padrão 2)
#   .\code\scripts\dev-cluster.ps1 gateway    sobe a API em http://127.0.0.1:8080 (chave dev-key; pesos em code/models/smollm2)
#   .\code\scripts\dev-cluster.ps1 cli <args>   roda `aic cluster <args>` contra esse cluster
#   .\code\scripts\dev-cluster.ps1 metrics      lê o /metrics do Prometheus (com o token)
#   .\code\scripts\dev-cluster.ps1 down         derruba tudo
#
# Exemplos:  ... cli dashboard --watch 2   |   ... cli benchmark --size 256 --repetitions 8
#
# Sem TLS (AI_CLUSTER_INSECURE=1) e com aprovação automática: é para a máquina de
# desenvolvimento, não para uma rede aberta.
param(
    [Parameter(Position = 0)] [string] $Action = '',
    [Parameter(Position = 1, ValueFromRemainingArguments = $true)] [string[]] $Rest = @()
)

$Net = 'aic-net'
$Img = if ($env:AIC_RUST_IMAGE) { $env:AIC_RUST_IMAGE } else { 'rust:1-bookworm' }
$R = '/t/release'
$Join = 'dev-token'
$Operator = 'dev-operator'
$Inference = 'dev-inference'
$Code = Split-Path -Parent $PSScriptRoot
$Self = $MyInvocation.MyCommand.Path

$Common = @(
    '--network', $Net, '-v', 'aic-target:/t', '-e', 'AI_CLUSTER_INSECURE=1',
    '-e', "AI_CLUSTER_JOIN_TOKEN=$Join", '-e', "AI_CLUSTER_OPERATOR_TOKEN=$Operator"
)

switch ($Action) {
    'build' {
        docker run --rm -v "${Code}:/w" -v aic-target:/t -e CARGO_TARGET_DIR=/t -w /w $Img cargo build --release --workspace
    }
    'up' {
        $Workers = if ($Rest.Count -gt 0) { [int]$Rest[0] } else { 2 }
        docker network create $Net 2>$null | Out-Null
        docker rm -f aic-controller 2>$null | Out-Null
        docker run -d --name aic-controller @Common -e AI_CLUSTER_AUTO_APPROVE=1 -e AI_CLUSTER_INFERENCE_TOKEN=$Inference -e AI_CLUSTER_METRICS_ADDR=0.0.0.0:9101 -p 9101:9101 $Img sh -c "exec $R/ai-cluster-controller" | Out-Null
        if ($LASTEXITCODE -ne 0) { exit 1 }
        foreach ($i in 1..$Workers) {
            docker rm -f "aic-worker-$i" 2>$null | Out-Null
            docker run -d --name "aic-worker-$i" @Common -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 -e "AI_CLUSTER_NODE_ID=worker-$i" $Img sh -c "exec $R/ai-cluster-worker" | Out-Null
        }
        Write-Host "controller e $Workers workers no ar. Tente:"
        Write-Host "  $Self cli nodes"
        Write-Host "  $Self cli dashboard --watch 2"
        Write-Host "  $Self cli benchmark --size 256 --repetitions 8"
    }
    'gateway' {
        docker rm -f aic-gateway 2>$null | Out-Null
        $Models = @()
        if (Test-Path "$Code\models\smollm2") { $Models = @('-v', "${Code}\models\smollm2:/model:ro", '-e', 'AI_CLUSTER_SMOLLM2_DIR=/model', '-e', 'AI_CLUSTER_SMOLLM2_SUBDIR=smollm2') }
        docker run -d --name aic-gateway @Common -e AI_CLUSTER_INFERENCE_TOKEN=$Inference -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 -e AI_CLUSTER_API_KEY=dev-key -e AI_CLUSTER_GATEWAY_ADDR=0.0.0.0:8080 -p 8080:8080 @Models $Img sh -c "exec $R/ai-cluster-gateway" | Out-Null
        Write-Host 'gateway em http://127.0.0.1:8080 (chave dev-key; o modelo leva ~30 s para carregar).'
    }
    'cli' {
        docker run --rm @Common -e AI_CLUSTER_CONTROLLER=http://aic-controller:50051 $Img sh -c "$R/aic cluster $($Rest -join ' ')"
    }
    'metrics' {
        docker run --rm @Common $Img curl -s -H "Authorization: Bearer $Operator" http://aic-controller:9101/metrics
    }
    'down' {
        $ids = docker ps -aq --filter 'name=aic-worker-'
        if ($ids) { docker rm -f $ids | Out-Null }
        docker rm -f aic-controller aic-gateway 2>$null | Out-Null
        Write-Host 'derrubado'
    }
    default {
        Get-Content $Self -TotalCount 12 | Select-Object -Skip 1
    }
}
