# TouchPad Remote — Android

Aplicativo Android em **Kotlin** com **Jetpack Compose** e **Material 3** que transforma o celular em um touchpad sem fio em tela cheia para controlar o Windows (inclusive em jogos 3D como Minecraft).

> **Servidor Windows obrigatório:** Para o aplicativo funcionar, execute o programa companheiro no PC Windows:
> **[TouchPadRemote-Windows](https://github.com/Henzogabriel954/TouchPadRemote-Windows)** ([baixar última versão para Windows](https://github.com/Henzogabriel954/TouchPadRemote-Windows/releases/latest)).

---

## Download do APK

Baixe o instalador Android diretamente na página oficial de Releases:

- **[Baixar APK mais recente (`TouchPadRemote-Android-v1.0.0.apk`)](https://github.com/Henzogabriel954/TouchPadRemote-Android/releases/latest)**

---

## Instalação no Android

1. Acesse **[Releases](https://github.com/Henzogabriel954/TouchPadRemote-Android/releases/latest)** pelo navegador do celular e baixe **`TouchPadRemote-Android-v1.0.0.apk`**.
2. Abra o arquivo `.apk` baixado (se o Android solicitar permissão para instalar aplicativos de fontes desconhecidas/navegador, autorize).
3. Conclua a instalação e abra o aplicativo **TouchPad Remote**.

**Requisitos:**
- Android 7.0+ (`minSdk 24`).
- Celular e computador Windows conectados na **mesma rede Wi-Fi / LAN** (comunicação UDP de baixa latência na porta padrão `45454`).

---

## Cadastro e Pareamento

1. Abra o aplicativo **TouchPad Remote** no Windows.
2. No celular, escolha uma das opções na tela inicial:
   - **Buscar na Rede**: envia broadcast UDP na rede local (porta `45454`) para encontrar computadores ativos automaticamente; toque no PC encontrado para preencher os campos.
   - **Cadastrar Computador**: informe manualmente **Nome do Computador**, **Endereço IP** e **Porta** (padrão `45454`) e toque em **Salvar**.
3. Cada computador salvo aparece em um quadrado na tela inicial com indicador de status (**Online** / **Offline**), atualizado automaticamente a cada 2,5 segundos ou pelo botão de atualizar.
4. Toque no quadrado de um computador **Online**:
   - Se o PC estiver offline ou inacessível, o aplicativo exibe o alerta **"Computador Offline"** e não abre o touchpad.
   - Se estiver online e aguardando aprovação, o aplicativo exibe **"Aceite o Pareamento no Windows"**.
5. Na janela do **TouchPad Remote** no Windows, clique no botão verde **Aceitar Pareamento**. Assim que aprovado, o touchpad tela cheia abre automaticamente no celular.

*(Caso a sessão seja recusada ou desconectada no computador Windows, o aplicativo retorna automaticamente à tela inicial e informa o ocorrido.)*

---

## Controles no Touchpad Tela Cheia

O modo touchpad utiliza uma superfície preta imersiva em tela cheia (com suporte a recorte de tela/notch, tela sempre ativa e alternância entre **Paisagem** e **Retrato** no botão de rotação):

| Gesto | Ação no Windows |
| --- | --- |
| **Deslizar 1 dedo** | Move o cursor do mouse em tempo real (baixa latência via UDP). |
| **Toque rápido no lado esquerdo (`ESQ`)** | Clique esquerdo rápido (`CLICK|LEFT`) com retorno tátil e efeito visual. |
| **Toque rápido no lado direito (`DIR`)** | Clique direito rápido (`CLICK|RIGHT`) com retorno tátil e efeito visual. |
| **Manter 1 dedo pressionado (260 ms)** | Pressiona e segura o botão correspondente (`ESQ` ou `DIR`), exibindo o indicador *"Segurando Clique Esquerdo/Direito"*. Permite arrastar janelas ou segurar cliques em jogos sem atrapalhar toques rápidos; solta automaticamente ao levantar o dedo. |
| **Deslizar 2 dedos verticalmente** | Rolagem vertical (`SCROLL`). |
| **Botão `Sair` (canto superior direito) ou Voltar** | Libera eventuais botões pressionados e volta para a lista de computadores. |

---

## Métodos de Conexão

- **Via Rede (Wi-Fi / LAN)** — **Ativo**: comunicação UDP na porta `45454`.
- **Bluetooth** — *Em breve*.
- **Cabo USB (ADB)** — *Em breve*.

---

## Estrutura do Projeto

- **`shared-protocol/`**: biblioteca Kotlin JVM compartilhada com a definição dos pacotes UDP (`TouchpadPacket`), enum de métodos (`ConnectionMethod`), codificador/decodificador (`TouchpadCodec`) e testes unitários (`TouchpadPacketTest`).
- **`android-app/`**: aplicativo Android (`MainActivity`, `ComputerListScreen`, `TouchpadFullscreenScreen`, `TouchpadUdpClient`, `ComputerRepository`, `ComputerProfile`).

---

## Como Compilar Localmente

### Pré-requisitos
- **JDK 17**
- **Android SDK** (`compileSdk 34`, `minSdk 24`, `targetSdk 34`)
- **Gradle 8.13** (ou **Android Studio**)

### Comandos

Executar os testes unitários do protocolo:

```bash
gradle :shared-protocol:test
```

Gerar o APK de debug:

```bash
gradle :android-app:assembleDebug
```

O APK compilado será salvo em:

```text
android-app/build/outputs/apk/debug/android-app-debug.apk
```
