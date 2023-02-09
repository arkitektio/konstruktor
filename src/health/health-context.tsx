import React, { useContext } from 'react'

export type HealthyJSON = { [element: string]: string }
export type DeadJSON = { Connection: string; [element: string]: string }

export type HealthReturn = {
  name: string
  reachable?: boolean
  ok?: HealthyJSON
  error?: DeadJSON
}

export type ContainerState = {
  state?: string
  status?: string
  image?: string
}

export type DeploymentState = {
  healthy: boolean
}

export type ArkitektState = DeploymentState & {
  postgres?: ContainerState
  redis?: ContainerState
  rabbit?: ContainerState
  service?: ContainerState
}

export type FlussState = DeploymentState & {
  postgres?: ContainerState
  service?: ContainerState
  agent?: ContainerState
}

export type HerreState = DeploymentState & {
  postgres?: ContainerState
  service?: ContainerState
}

export type ElementsState = DeploymentState & {
  postgres?: ContainerState
  service?: ContainerState
  storage?: ContainerState
}

export type PortState = DeploymentState & {
  service?: ContainerState
  agent?: ContainerState
}

export type GenericaState = DeploymentState & {
  agent?: ContainerState
}

export type MeshState = {
  arkitekt: ArkitektState
  fluss: FlussState
  herre: HerreState
  elements: ElementsState
  port: PortState
  generica: GenericaState
}

export type ArkitektServiceState = HealthReturn

export type ServiceState = {
  arkitekt?: HealthReturn
  herre?: HealthReturn
  elements?: HealthReturn
  fluss?: HealthReturn
  port?: HealthReturn
}

export interface HealthContextType {
  service?: ServiceState
}

export const HealthContext = React.createContext<HealthContextType>({
  service: null as unknown as ServiceState,
})

export const useHealth = () => useContext(HealthContext)
