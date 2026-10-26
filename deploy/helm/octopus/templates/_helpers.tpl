{{/* Expand the name of the chart. */}}
{{- define "octopus.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* Fully qualified app name. */}}
{{- define "octopus.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "octopus.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* Common labels. */}}
{{- define "octopus.labels" -}}
helm.sh/chart: {{ include "octopus.chart" . }}
{{ include "octopus.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{/* Selector labels. */}}
{{- define "octopus.selectorLabels" -}}
app.kubernetes.io/name: {{ include "octopus.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/* Service account name. */}}
{{- define "octopus.serviceAccountName" -}}
{{- if .Values.serviceAccount.create -}}
{{- default (include "octopus.fullname" .) .Values.serviceAccount.name -}}
{{- else -}}
{{- default "default" .Values.serviceAccount.name -}}
{{- end -}}
{{- end -}}

{{/* Image reference (tag defaults to appVersion). */}}
{{- define "octopus.image" -}}
{{- $tag := .Values.image.tag | default .Chart.AppVersion -}}
{{- printf "%s:%s" .Values.image.repository $tag -}}
{{- end -}}

{{/*
Port of the private admin listener, taken from config.admin.listen
("0.0.0.0:9090" or "[::]:9090" -> 9090). Empty when admin.listen is unset.
*/}}
{{- define "octopus.adminListenPort" -}}
{{- $listen := dig "admin" "listen" "" .Values.config | toString -}}
{{- if $listen -}}
{{- splitList ":" $listen | last -}}
{{- end -}}
{{- end -}}
