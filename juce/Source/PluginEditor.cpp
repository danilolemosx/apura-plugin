#include "PluginEditor.h"

#include "BinaryData.h"

using namespace juce;

static std::vector<const char*> idsSliders()
{
    std::vector<const char*> v { ID::target, ID::ceiling, ID::highPass, ID::width, ID::gain };
    for (int k = 0; k < 4; ++k)
    {
        v.push_back (ID::freq[k]);
        v.push_back (ID::ganho[k]);
    }
    return v;
}
static const char* const idsChaves[] = { ID::listen, ID::compare };
static const char* const idsListas[] = { ID::destination, ID::character };

ApuraEditor::ApuraEditor (ApuraProcessor& p)
    : AudioProcessorEditor (&p), processador (p)
{
    const auto ids = idsSliders();
    for (auto* id : ids)
        sliders.push_back (std::make_unique<WebSliderRelay> (id));
    for (auto* id : idsChaves)
        chaves.push_back (std::make_unique<WebToggleButtonRelay> (id));
    for (auto* id : idsListas)
        listas.push_back (std::make_unique<WebComboBoxRelay> (id));

    pagina = std::make_unique<WebBrowserComponent> (opcoes());

    for (size_t i = 0; i < sliders.size(); ++i)
        ligacoesSliders.push_back (std::make_unique<WebSliderParameterAttachment> (
            *processador.state.getParameter (ids[i]), *sliders[i], nullptr));
    for (size_t i = 0; i < chaves.size(); ++i)
        ligacoesChaves.push_back (std::make_unique<WebToggleButtonParameterAttachment> (
            *processador.state.getParameter (idsChaves[i]), *chaves[i], nullptr));
    for (size_t i = 0; i < listas.size(); ++i)
        ligacoesListas.push_back (std::make_unique<WebComboBoxParameterAttachment> (
            *processador.state.getParameter (idsListas[i]), *listas[i], nullptr));

    addAndMakeVisible (*pagina);
    pagina->goToURL (WebBrowserComponent::getResourceProviderRoot());

    setSize (760, 500);
    startTimerHz (15);
}

ApuraEditor::~ApuraEditor()
{
    stopTimer();
}

WebBrowserComponent::Options ApuraEditor::opcoes()
{
    auto o = WebBrowserComponent::Options {}
                 .withBackend (WebBrowserComponent::Options::Backend::webview2)
                 .withWinWebView2Options (WebBrowserComponent::Options::WinWebView2 {}
                                              .withUserDataFolder (File::getSpecialLocation (File::SpecialLocationType::tempDirectory)))
                 .withNativeIntegrationEnabled()
                 .withKeepPageLoadedWhenBrowserIsHidden()
                 .withResourceProvider ([this] (const auto& url) { return recurso (url); })
                 // a página avisa que carregou; com APURA_TESTE_JANELA=<arquivo> isso vira um
                 // arquivo de conferência (usado pelos testes, que não podem capturar a tela)
                 .withNativeFunction ("janelaPronta", [this] (const Array<var>& args, auto completar)
                 {
                     const auto arquivo = SystemStats::getEnvironmentVariable ("APURA_TESTE_JANELA", {});
                     if (arquivo.isNotEmpty())
                         File (arquivo).replaceWithText (args.isEmpty() ? String() : args[0].toString());
                     // a página acabou de carregar: manda o que já houver
                     versaoEnviada = -1;
                     versaoAprendidoEnviada = -1;
                     tiquesDesdeEstado = 1000;
                     completar (var());
                 })
                 // MATCH: os 4 pontos onde a análise sugere (devolve false se ainda não há análise)
                 .withNativeFunction ("match", [this] (const Array<var>&, auto completar)
                 {
                     completar (var (processador.aplicarMatch()));
                 })
                 // destino escolhido: alvo e teto do destino
                 .withNativeFunction ("destino", [this] (const Array<var>& args, auto completar)
                 {
                     if (! args.isEmpty())
                         processador.escolherDestino ((int) args[0]);
                     completar (var());
                 });
    for (auto& s : sliders)
        o = o.withOptionsFrom (*s);
    for (auto& c : chaves)
        o = o.withOptionsFrom (*c);
    for (auto& l : listas)
        o = o.withOptionsFrom (*l);
    return o;
}

std::optional<WebBrowserComponent::Resource> ApuraEditor::recurso (const String& url)
{
    const auto nome = url == "/" ? String ("index.html") : url.fromFirstOccurrenceOf ("/", false, false);

    struct Arquivo { const char* url; const char* dados; int tamanho; const char* mime; };
    const Arquivo arquivos[] = {
        { "index.html", BinaryData::index_html, BinaryData::index_htmlSize, "text/html" },
        { "style.css", BinaryData::style_css, BinaryData::style_cssSize, "text/css" },
        { "app.js", BinaryData::app_js, BinaryData::app_jsSize, "text/javascript" },
        { "fonts/archivo.woff2", BinaryData::archivo_woff2, BinaryData::archivo_woff2Size, "font/woff2" },
    };
    // o juce.js do JUCE é um módulo (export); servimos como script comum que expõe window.Juce
    if (nome == "juce.js")
    {
        auto codigo = String::fromUTF8 (BinaryData::index_js, BinaryData::index_jsSize)
                          .replace ("import \"./check_native_interop.js\";", "")
                          .replace ("export {", "window.Juce = {");
        codigo = "if (window.__JUCE__) {\n" + codigo + "\n}\n";
        const auto utf8 = codigo.toUTF8();
        const auto* d = reinterpret_cast<const std::byte*> (utf8.getAddress());
        return WebBrowserComponent::Resource { std::vector<std::byte> (d, d + utf8.sizeInBytes() - 1), String ("text/javascript") };
    }
    for (const auto& a : arquivos)
        if (nome == a.url)
            return WebBrowserComponent::Resource { std::vector<std::byte> ((const std::byte*) a.dados, (const std::byte*) a.dados + a.tamanho),
                                                   String (a.mime) };
    return std::nullopt;
}

static var lista (const float* v, int n)
{
    Array<var> a;
    for (int i = 0; i < n; ++i)
        a.add (std::isfinite (v[i]) ? var ((double) v[i]) : var());
    return a;
}

static var numero (float v)
{
    return std::isfinite (v) ? var ((double) v) : var();
}

void ApuraEditor::enviarMedida()
{
    const auto m = processador.ultimaMedida();
    const auto& r = m.resultado;
    DynamicObject::Ptr d (new DynamicObject());
    d->setProperty ("analisado", m.erro == 0 && m.canais > 0);
    d->setProperty ("erro", m.erro);
    d->setProperty ("canais", m.canais);
    d->setProperty ("taxa", m.taxa);
    d->setProperty ("segundos", m.segundos);
    d->setProperty ("tempo", m.tempoProcesso);
    d->setProperty ("lufsIn", numero (r.lufs_entrada));
    d->setProperty ("lufsOut", numero (r.lufs_saida));
    d->setProperty ("tpIn", numero (r.tp_entrada));
    d->setProperty ("tpOut", numero (r.tp_saida));
    d->setProperty ("lraIn", numero (r.lra_entrada));
    d->setProperty ("lraOut", numero (r.lra_saida));
    d->setProperty ("ganho", numero (r.ganho_db));
    d->setProperty ("noTeto", r.no_teto_de_ganho != 0);
    d->setProperty ("comp", numero (r.compressor_max_db));
    d->setProperty ("lim", numero (r.limitador_max_db));
    d->setProperty ("espIn", lista (r.espectro_entrada_db, 30));
    d->setProperty ("espOut", lista (r.espectro_saida_db, 30));

    // com que ajustes o clip foi feito (a janela avisa se mudaram depois)
    const auto& a = m.ajustes;
    DynamicObject::Ptr aj (new DynamicObject());
    aj->setProperty ("alvo", a.alvo_lufs);
    aj->setProperty ("teto", a.teto_db);
    aj->setProperty ("hp", a.passa_altas_hz);
    aj->setProperty ("width", a.width_db);
    aj->setProperty ("carater", a.carater);
    aj->setProperty ("comparar", a.comparar);
    aj->setProperty ("freq", lista (a.bandas_freq, 4));
    aj->setProperty ("ganho", lista (a.bandas_ganho, 4));
    d->setProperty ("ajustes", var (aj.get()));

    pagina->emitEventIfBrowserIsVisible ("medida", var (d.get()));
}

void ApuraEditor::enviarAoVivo()
{
    const auto m = processador.medidasAoVivo();
    DynamicObject::Ptr d (new DynamicObject());
    d->setProperty ("momentaneo", numero (m.momentaneo));
    d->setProperty ("curto", numero (m.curto));
    d->setProperty ("integrado", numero (m.integrado));
    d->setProperty ("pico", numero (m.pico_db));
    d->setProperty ("lim", numero (m.limitador_db));
    d->setProperty ("comp", numero (m.compressor_db));
    d->setProperty ("espIn", lista (m.espectro_entrada, 30));
    d->setProperty ("espOut", lista (m.espectro_saida, 30));
    d->setProperty ("escutandoS", numero (m.escutando_s));
    d->setProperty ("lufsEntradaEscuta", numero (m.lufs_entrada_escuta));
    d->setProperty ("lufsPreEscuta", numero (m.lufs_pre_escuta));
    pagina->emitEventIfBrowserIsVisible ("aoVivo", var (d.get()));
}

void ApuraEditor::enviarAprendizado()
{
    const auto a = processador.aprendido();
    DynamicObject::Ptr d (new DynamicObject());
    d->setProperty ("valido", a.valido);
    d->setProperty ("segundos", numero (a.segundos));
    d->setProperty ("lufsEntrada", numero (a.lufsEntrada));
    d->setProperty ("lufsPre", numero (a.lufsPre));
    d->setProperty ("lufsSaida", numero (a.lufsSaida));
    d->setProperty ("ganhoSugerido", numero (a.ganhoSugerido));
    d->setProperty ("alvo", numero (a.alvo));
    pagina->emitEventIfBrowserIsVisible ("aprendizado", var (d.get()));
}

void ApuraEditor::enviarEstado()
{
    DynamicObject::Ptr d (new DynamicObject());
    d->setProperty ("audiosuite", audioSuiteEnviado);
    d->setProperty ("processando", processandoEnviado);
    d->setProperty ("latenciaMs", processador.latenciaMs());
    pagina->emitEventIfBrowserIsVisible ("estado", var (d.get()));
}

void ApuraEditor::timerCallback()
{
    const int versao = processador.versaoMedida.load();
    if (versao != versaoEnviada)
    {
        versaoEnviada = versao;
        if (versao > 0)
            enviarMedida();
    }
    const int vivo = processador.versaoAoVivo.load();
    if (vivo != versaoAoVivoEnviada)
    {
        versaoAoVivoEnviada = vivo;
        enviarAoVivo();
    }
    const int aprendido = processador.versaoAprendido.load();
    if (aprendido != versaoAprendidoEnviada)
    {
        versaoAprendidoEnviada = aprendido;
        enviarAprendizado();
    }

    const bool audioSuite = processador.estaNoAudioSuite();
    const bool processando = processador.processando.load();
    if (audioSuite != audioSuiteEnviado || processando != processandoEnviado || ++tiquesDesdeEstado >= 15)
    {
        audioSuiteEnviado = audioSuite;
        processandoEnviado = processando;
        tiquesDesdeEstado = 0;
        enviarEstado();
    }
}

void ApuraEditor::paint (Graphics& g)
{
    g.fillAll (Colour (0xff1d1c1b));
}

void ApuraEditor::resized()
{
    pagina->setBounds (getLocalBounds());
}
