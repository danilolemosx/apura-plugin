// Apura: janela do plugin (página HTML em ui/, ligada aos parâmetros).

#pragma once

#include <juce_gui_extra/juce_gui_extra.h>

#include "PluginProcessor.h"

#include <memory>
#include <vector>

class ApuraEditor : public juce::AudioProcessorEditor, private juce::Timer
{
public:
    explicit ApuraEditor (ApuraProcessor&);
    ~ApuraEditor() override;

    void resized() override;
    void paint (juce::Graphics&) override;

private:
    void timerCallback() override;
    void enviarMedida();
    void enviarAoVivo();
    void enviarAprendizado();
    void enviarEstado();
    std::optional<juce::WebBrowserComponent::Resource> recurso (const juce::String& url);
    juce::WebBrowserComponent::Options opcoes();

    ApuraProcessor& processador;

    // um "relay" por parâmetro (nome = ID do parâmetro, usado pelo app.js)
    std::vector<std::unique_ptr<juce::WebSliderRelay>> sliders;
    std::vector<std::unique_ptr<juce::WebToggleButtonRelay>> chaves;
    std::vector<std::unique_ptr<juce::WebComboBoxRelay>> listas;

    std::unique_ptr<juce::WebBrowserComponent> pagina;

    std::vector<std::unique_ptr<juce::WebSliderParameterAttachment>> ligacoesSliders;
    std::vector<std::unique_ptr<juce::WebToggleButtonParameterAttachment>> ligacoesChaves;
    std::vector<std::unique_ptr<juce::WebComboBoxParameterAttachment>> ligacoesListas;

    int versaoEnviada = 0, versaoAoVivoEnviada = 0, versaoAprendidoEnviada = -1;
    bool audioSuiteEnviado = false, processandoEnviado = false;
    int tiquesDesdeEstado = 0;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (ApuraEditor)
};
