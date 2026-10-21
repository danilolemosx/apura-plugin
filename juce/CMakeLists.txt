# Apura (AAX AudioSuite) com JUCE 8 + o remendo de clip inteiro (scripts/preparar-juce.sh)
#
#   scripts/preparar-juce.sh
#   cmake -B build -S juce -DCMAKE_BUILD_TYPE=Release
#   cmake --build build --config Release --target Apura_AAX
#
# O motor em Rust (../motor) é compilado pelo cargo antes do plugin.
# Formatos: AAX (Media Composer/Pro Tools: master ao vivo e AudioSuite de clip inteiro),
# VST3 e AU (Premiere, Resolve, Logic: ao vivo, no master). No Windows, VST3 e AAX.

cmake_minimum_required(VERSION 3.22)
option(APURA_UNIVERSAL "Mac: compilar para Apple Silicon e Intel" OFF)
set(CMAKE_OSX_DEPLOYMENT_TARGET "11.0" CACHE STRING "")
if(APURA_UNIVERSAL)
	set(CMAKE_OSX_ARCHITECTURES "arm64;x86_64" CACHE STRING "" FORCE)
else()
	set(CMAKE_OSX_ARCHITECTURES "arm64" CACHE STRING "")
endif()
project(ApuraPlugin VERSION 0.2.0 LANGUAGES C CXX)

set(JUCE_DIR "$ENV{HOME}/.cache/apura-aax/JUCE" CACHE PATH "Pasta do JUCE 8 com o remendo do Apura")
if(NOT EXISTS "${JUCE_DIR}/modules/juce_audio_processors/utilities/juce_AAXClientExtensions.h")
	message(FATAL_ERROR "JUCE não encontrado em ${JUCE_DIR}: rode scripts/preparar-juce.sh")
endif()
file(READ "${JUCE_DIR}/modules/juce_audio_processors/utilities/juce_AAXClientExtensions.h" EXTENSOES)
if(NOT EXTENSOES MATCHES "usesWholeClipAudioSuite")
	message(FATAL_ERROR "O JUCE em ${JUCE_DIR} está sem o remendo do Apura: rode scripts/preparar-juce.sh")
endif()
add_subdirectory("${JUCE_DIR}" JUCE EXCLUDE_FROM_ALL)

# --- motor (Rust) ---------------------------------------------------------------
set(MOTOR_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../motor")
# fora da pasta do projeto (o caminho tem espaços)
set(MOTOR_TARGET_DIR "$ENV{HOME}/.cache/apura-aax/target" CACHE PATH "Pasta de compilação do motor")
set(CARGO_BUILD cargo build --release --lib --manifest-path "${MOTOR_DIR}/Cargo.toml" --target-dir "${MOTOR_TARGET_DIR}")

if(WIN32)
	set(MOTOR_LIB "${MOTOR_TARGET_DIR}/release/apura_motor.lib")
	add_custom_target(motor
		COMMAND ${CARGO_BUILD}
		BYPRODUCTS "${MOTOR_LIB}"
		WORKING_DIRECTORY "${MOTOR_DIR}"
		COMMENT "Compilando o motor (Rust)"
		USES_TERMINAL)
elseif(APURA_UNIVERSAL)
	set(MOTOR_LIB "${MOTOR_TARGET_DIR}/universal/libapura_motor.a")
	add_custom_target(motor
		COMMAND ${CMAKE_COMMAND} -E env MACOSX_DEPLOYMENT_TARGET=${CMAKE_OSX_DEPLOYMENT_TARGET} ${CARGO_BUILD} --target aarch64-apple-darwin
		COMMAND ${CMAKE_COMMAND} -E env MACOSX_DEPLOYMENT_TARGET=${CMAKE_OSX_DEPLOYMENT_TARGET} ${CARGO_BUILD} --target x86_64-apple-darwin
		COMMAND ${CMAKE_COMMAND} -E make_directory "${MOTOR_TARGET_DIR}/universal"
		COMMAND lipo -create
			"${MOTOR_TARGET_DIR}/aarch64-apple-darwin/release/libapura_motor.a"
			"${MOTOR_TARGET_DIR}/x86_64-apple-darwin/release/libapura_motor.a"
			-output "${MOTOR_LIB}"
		BYPRODUCTS "${MOTOR_LIB}"
		WORKING_DIRECTORY "${MOTOR_DIR}"
		COMMENT "Compilando o motor (Rust, Apple + Intel)"
		USES_TERMINAL)
else()
	set(MOTOR_LIB "${MOTOR_TARGET_DIR}/release/libapura_motor.a")
	add_custom_target(motor
		COMMAND ${CMAKE_COMMAND} -E env MACOSX_DEPLOYMENT_TARGET=${CMAKE_OSX_DEPLOYMENT_TARGET} ${CARGO_BUILD}
		BYPRODUCTS "${MOTOR_LIB}"
		WORKING_DIRECTORY "${MOTOR_DIR}"
		COMMENT "Compilando o motor (Rust)"
		USES_TERMINAL)
endif()

# --- plugin ---------------------------------------------------------------------
if(APPLE)
	set(APURA_FORMATOS AAX VST3 AU)
else()
	set(APURA_FORMATOS AAX VST3)
endif()

juce_add_plugin(Apura
	COMPANY_NAME "Danilo Lemos"
	BUNDLE_ID "br.danilolemos.apura"
	PLUGIN_MANUFACTURER_CODE DnLm
	PLUGIN_CODE Apur
	FORMATS ${APURA_FORMATOS}
	AAX_IDENTIFIER br.danilolemos.apura
	AAX_CATEGORY Dynamics
	PRODUCT_NAME "Apura"
	DESCRIPTION "Whole-clip loudness and mastering for delivery"
	IS_SYNTH FALSE
	NEEDS_MIDI_INPUT FALSE
	NEEDS_MIDI_OUTPUT FALSE
	IS_MIDI_EFFECT FALSE
	NEEDS_WEB_BROWSER TRUE
	NEEDS_WEBVIEW2 TRUE
	VST3_CATEGORIES Fx Dynamics
	AU_MAIN_TYPE kAudioUnitType_Effect
	COPY_PLUGIN_AFTER_BUILD FALSE)

juce_add_binary_data(ApuraUI SOURCES
	ui/index.html
	ui/style.css
	ui/app.js
	ui/fonts/archivo.woff2
	"${JUCE_DIR}/modules/juce_gui_extra/native/javascript/index.js")

target_sources(Apura PRIVATE
	Source/PluginProcessor.cpp
	Source/PluginEditor.cpp)

target_compile_definitions(Apura PUBLIC
	JUCE_WEB_BROWSER=1
	JUCE_USE_CURL=0
	JUCE_VST3_CAN_REPLACE_VST2=0)
if(WIN32)
	# janela em HTML no Windows: WebView2 (Edge), ligado estaticamente
	target_compile_definitions(Apura PUBLIC JUCE_USE_WIN_WEBVIEW2_WITH_STATIC_LINKING=1)
endif()

add_dependencies(Apura motor)
target_link_libraries(Apura
	PRIVATE
		ApuraUI
		"${MOTOR_LIB}"
		juce::juce_audio_utils
		juce::juce_gui_extra
	PUBLIC
		juce::juce_recommended_config_flags
		juce::juce_recommended_lto_flags
		juce::juce_recommended_warning_flags)
if(WIN32)
	# bibliotecas do sistema que a biblioteca padrão do Rust usa
	set(SISTEMA_RUST ws2_32 userenv ntdll bcrypt advapi32 kernel32 synchronization)
	target_link_libraries(Apura PRIVATE ${SISTEMA_RUST})
endif()

# --- testes ---------------------------------------------------------------------
# testa-apura: o processador direto (sem host), como o AudioSuite chamaria, e a janela
# do VST3 aberta por alguns segundos.
juce_add_console_app(testa-apura PRODUCT_NAME "testa-apura")
target_sources(testa-apura PRIVATE teste/Main.cpp Source/PluginProcessor.cpp)
target_compile_definitions(testa-apura PRIVATE
	APURA_SEM_JANELA=1
	JUCE_PLUGINHOST_VST3=1
	JUCE_MODAL_LOOPS_PERMITTED=1
	JUCE_WEB_BROWSER=0
	JUCE_USE_CURL=0)
add_dependencies(testa-apura motor)
target_link_libraries(testa-apura PRIVATE
	"${MOTOR_LIB}"
	juce::juce_audio_processors
	juce::juce_audio_formats
	juce::juce_recommended_config_flags)
if(WIN32)
	target_link_libraries(testa-apura PRIVATE ${SISTEMA_RUST})
endif()
