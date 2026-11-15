# Nocto — documento de design

Status: **rascunho para aprovação**. Nenhum código foi escrito.

## 1. Intenção e porquê

Nocto é uma "Dynamic Island" para Windows: uma pílula translúcida no topo da tela que serve de assistente de uso diário. Com um atalho global, aparece uma barra de comando pequena (no estilo da janela rápida do Claude) e executa ações no computador: abrir apps, controlar o Spotify, consultar um calendário próprio, converter imagens arrastadas.

Por quê: o autor quer um assistente à mão enquanto trabalha, codifica ou joga, sem trocar de janela. Também é projeto de portfólio e deve rodar bem num notebook fraco de um amigo, não só no PC do autor (RTX 4050, Ryzen 7).

## 2. Objetivos, não-objetivos e anti-objetivos

**Objetivos**
- Muito leve: o app parado não pode ser percebido em jogo nem no trabalho.
- Atalho global, barra de comando com **comandos fixos**, executada com permissões.
- Ilha em repouso e expandida, com visual translúcido que muda por horário.
- Spotify: música atual, play/pause/próxima e "toca X".
- Calendário local com tarefas e compromissos.
- Arrastar arquivo para converter imagem.
- 100% gratuito: nenhum serviço pago, nenhuma conta obrigatória.

**Não-objetivos (por enquanto)**
- Linguagem livre com IA (local ou nuvem). Pode vir depois como módulo opcional.
- Integração com o INA (o INA é só referência de cor).
- E-mail, comando por voz, remover fundo, aba de personalização completa. Ficam para versões seguintes.

**Anti-objetivos (o que torna o projeto um fracasso mesmo que funcione)**
1. **Ficar pesado**: perder desempenho em jogos, no trabalho ou em qualquer outra situação. É a prioridade máxima.
2. **Atrapalhar o jogo** (roubar foco, aparecer sem ser chamada). Aparecer na hora errada em si é ajustável; atrapalhar o jogo não.

O design visual em si pode mudar depois sem custo; o orçamento de desempenho não.

## 3. Restrições

- Windows 11 é o alvo principal. No Windows 10 o app deve funcionar com translucidez simplificada.
- Rust, interface nativa (sem navegador embutido).
- Tudo gratuito, sem custos de licença, nuvem ou assinatura.
- O autor tem Spotify Premium (necessário para o controle de reprodução pela API oficial).
- Dados locais só, em arquivo no disco do usuário.

## 4. Orçamento de desempenho (metas a medir, não promessas)

| Item | Meta |
|---|---|
| RAM com a ilha parada | na casa de 10 a 30 MB |
| CPU parada | ~0%, sem loop de verificação |
| GPU com a ilha oculta/parada | nenhum quadro desenhado |
| Abertura da barra após o atalho | imperceptível (dezenas de ms) |
| Recursos pesados (ex.: modelo de IA) | só sob demanda, descarregados após o uso |

Regras de arquitetura que sustentam isso:
- Atalho global registrado via `RegisterHotKey` do Windows. O processo só acorda quando o atalho é pressionado. Não usar hooks de teclado globais (custam latência e são vistos como suspeitos por sistemas anti-cheat).
- Renderizar apenas quando houver animação ou mudança visível; zero quadros em repouso.
- Preferir renderizador por software enquanto a ilha estiver oculta ou simples, para não disputar a GPU com o jogo.
- Troca de tema por horário agendada para o próximo horário de virada, sem verificação periódica.
- Medir RAM/CPU/GPU com o app parado, com a barra aberta, e durante um jogo, antes de aceitar qualquer recurso novo.

## 5. Abordagens consideradas

| | Descrição | RAM parada (aprox.) | Troca |
|---|---|---|---|
| A | Tauri + WebView2 (interface web) | 50–150 MB | Mais fácil de embelezar; mais pesada. Contraria a prioridade máxima. |
| **B** | **Rust nativo com Slint** | **10–30 MB** | **Equilíbrio entre leveza e visual polido, com animações declarativas.** |
| C | Win32/Direct2D puros (`windows-rs`) | 5–10 MB | Mais leve; muito trabalho para um visual animado e difícil de crescer. |

**Escolhida: B**, pelo autor. Alternativa dentro de B: egui, mais simples, com visual polido mais trabalhoso.

## 6. Decisões já tomadas pelo autor

- Nome: **Nocto**.
- Linguagem: Rust, o mais leve possível.
- Interface: nativa (abordagem B).
- Cérebro da barra: **comandos fixos** por regras e correspondência de palavras (opção 1). IA fica para depois.
- Independente do INA.
- Ativação por atalho global, com barra de comando que aparece e some.
- Comportamento da ilha: repouso (pílula pequena), barra de comando e expandida (calendário etc.), como esboçado na conversa.

## 7. Visual

### 7.1 Tema por horário (horários propostos, ajustáveis)

| Período | Horário | Aparência |
|---|---|---|
| Dia | 05:00–15:59 | Vidro translúcido azul-céu claro |
| Pôr do sol | 16:00–17:59 | Vidro translúcido alaranjado |
| Noite | 18:00–04:59 | Vidro translúcido azul escuro, na família do INA (ex.: `#0b2a44` de fundo e `#2bb4ff` de destaque) |

A troca é um fade suave de alguns segundos. Haverá opção de forçar um tema fixo. Cores exatas e níveis de blur ficam para a etapa de interface.

### 7.2 Referências e o que aproveitar

- **Apple Design Resources** (developer.apple.com/design/resources): a página oferece UI kits, fontes (SF Pro) e SF Symbols; **não** tem seção específica de materiais/Liquid Glass. Para vidro e translucidez, a fonte provável é a Human Interface Guidelines da Apple (developer.apple.com/design/human-interface-guidelines), que ainda precisa ser lida. Aproveitar: ideias de hierarquia, espaçamento, ícones e linguagem de Liquid Glass.
- **Refero, estilo "Apple design system"** (tema claro): aproveitar só o que serve a uma pílula de interface:
  - Pílula como forma principal (raio 980px para botões) e raio de 8px para cartões e campos.
  - Base de espaçamento de 4px (4, 8, 12, 16, 20, 24).
  - Hierarquia por bordas de 1px e troca de superfície em vez de sombras pesadas.
  - Tipografia em escala enxuta (12, 14, 17, 21 px), pesos 400/600, tracking mais fechado em tamanhos pequenos.
  - Regra de um único azul de ação por elemento, sem misturar tons.
  - Focus ring de 2px nos campos.
  - **Não aproveitar**: layout de páginas web (seções, herói, 1440px), paleta clara como fundo (Nocto é translúcido) e valores de blur (o Refero não documenta nenhum).
- **A verificar antes de usar**: a licença das fontes SF Pro costuma restringir o uso a plataformas Apple. Para Windows, o plano é usar uma fonte livre ou nativa (Segoe UI Variable ou Inter) com a mesma escala. Confirmar antes de empacotar qualquer fonte.
- **Skills de design** instaladas (`ui-ux-pro-max`, `frontend-design`, `design-system`) podem ser usadas na etapa de interface para tokens, estados e acessibilidade.

## 8. Funcionalidades da primeira versão

1. **Ilha** em repouso e expandida, com tema por horário.
2. **Barra de comando** (atalho global): abrir apps, sites e pastas por comandos fixos. Ações sensíveis pedem confirmação; os comandos permitidos vêm de uma lista (allowlist).
3. **Spotify**:
   - Música atual, play/pause e próxima via controles de mídia do Windows (sem login).
   - "Toca X" via API oficial do Spotify (OAuth com PKCE, plano Premium).
   - Ponto a verificar: o modo de desenvolvimento do Spotify limita o número de usuários autorizados. O autor usa; um amigo precisaria ser adicionado, ou usaria só os controles locais.
4. **Calendário** local: clicar no dia, adicionar tarefa ou compromisso com horário, salvo em arquivo local.
5. **Conversão de imagem** por arrastar e soltar (PNG, JPEG, WebP) com bibliotecas Rust.

Fica para depois: remover fundo (modelo baixado sob demanda), e-mail (API do Gmail com OAuth), comando por voz, personalização completa.

## 9. Riscos e pontos de atenção

- **Jogos em tela cheia exclusiva**: sobreposições não aparecem sobre eles; funciona com janela sem borda ("borderless"), que é o modo mais comum hoje. Registrar como limitação conhecida.
- **Foco**: a barra de comando deve receber o teclado ao abrir e devolver o foco à janela anterior ao fechar, sem causar queda de quadros.
- **Windows 10**: translucidez simplificada.
- **Anti-cheat**: usar só `RegisterHotKey`, sem hooks nem injeção em outros processos.

## 10. O que fica para quem for construir

- Escolher entre o renderizador de Slint por software ou por GPU, medindo.
- Estrutura do analisador de comandos fixos e formato da lista de permissões.
- Formato do arquivo do calendário (JSON simples ou iCalendar).
- Detalhes do efeito de vidro (Acrylic/Mica via API do Windows) e valores finais de cor, blur e animação.
- Empacotamento e instalador.

## 11. Antes de codar

O autor avisou que quer trocar de modelo (**Fable 5.2**) para a parte de código. Nada é construído até este documento ser aprovado e o autor confirmar a troca de modelo.
