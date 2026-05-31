'use client'

import { ShaderGradient, ShaderGradientCanvas } from '@shadergradient/react'

/**
 * Violet shader-gradient backdrop for the hero. Isolated into its own module so
 * the hero can load it client-only via `next/dynamic({ ssr: false })` — the
 * WebGL/three internals must never run during SSR.
 */
export default function ShaderBg() {
  return (
    <ShaderGradientCanvas
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        width: '100%',
        height: '100%',
      }}
      lazyLoad={false}
      pixelDensity={1}
      pointerEvents="none"
    >
      <ShaderGradient
        animate="on"
        type="sphere"
        wireframe={false}
        shader="defaults"
        uTime={0}
        uSpeed={0.3}
        uStrength={0.3}
        uDensity={0.8}
        uFrequency={5.5}
        uAmplitude={3.2}
        positionX={-0.1}
        positionY={0}
        positionZ={0}
        rotationX={0}
        rotationY={130}
        rotationZ={70}
        color1="#c084fc"
        color2="#a855f7"
        color3="#6d28d9"
        reflection={0.4}
        cAzimuthAngle={270}
        cPolarAngle={180}
        cDistance={0.5}
        cameraZoom={15.1}
        lightType="env"
        brightness={0.85}
        envPreset="city"
        grain="on"
        toggleAxis={false}
        zoomOut={false}
        hoverState=""
        enableTransition={false}
      />
    </ShaderGradientCanvas>
  )
}
