#!/bin/sh
# Prepara o JUCE 8.0.9 com o remendo do AudioSuite de clip inteiro (juce/patches/).
#
#   scripts/preparar-juce.sh [pasta do JUCE]      padrão: ~/.cache/apura-aax/JUCE
#
# O remendo dá ao plug-in AAX um Host Processor: no AudioSuite o Avid entrega a seleção
# inteira, com todos os canais juntos, antes de processar (ver AAXClientExtensions).
set -e
cd "$(dirname "$0")/.."
DESTINO="${1:-$HOME/.cache/apura-aax/JUCE}"
REMENDO="$PWD/juce/patches/juce-8.0.9-audiosuite-clip-inteiro.patch"

if [ ! -d "$DESTINO/.git" ]; then
	# sem converter fins de linha (o remendo tem CRLF, como os arquivos do JUCE)
	git -c core.autocrlf=false clone --depth 1 --branch 8.0.9 https://github.com/juce-framework/JUCE.git "$DESTINO"
fi
cd "$DESTINO"
if git apply --reverse --check "$REMENDO" 2>/dev/null; then
	echo "JUCE pronto (remendo já aplicado): $DESTINO"
else
	git apply "$REMENDO"
	echo "JUCE pronto (remendo aplicado): $DESTINO"
fi
