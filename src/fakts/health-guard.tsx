import React, { useEffect, useState } from 'react'
import { useFakts } from './fakts-config'

export type HealthReturn = {
  name: string
  ok?: HealthyJSON
  error?: DeadJSON
}

export type HealthyJSON = { [element: string]: string }

export type DeadJSON = {
  Connection: string
  [element: string]: string
}

export const checkHealth = async (
  name: string,
  healthz: string
): Promise<HealthReturn> => {
  try {
    const res = await fetch(`${healthz}/?format=json`, {
      headers: {
        'Content-Type': 'application/json',
      },
    })

    if (res.ok) return { name, ok: (await res.json()) as HealthyJSON }
    else {
      return { name, error: (await res.json()) as DeadJSON }
    }
  } catch (e) {
    return { name, error: { Connection: (e as any).message } }
  }
}

export const HealthGuard: React.FC<{
  services: string[]
  fallback?: any
  children: React.ReactNode
}> = ({ services, children, fallback: Fallback }) => {
  const { fakts } = useFakts()
  const [errors, setErrors] = useState<HealthReturn[] | undefined>()

  useEffect(() => {
    if (fakts) {
      try {
        let checks = []

        for (let key in services) {
          let value = fakts[key as keyof any] as any
          if (value.healthz) {
            checks.push(checkHealth(key, value.healthz))
          }
        }

        Promise.all(checks).then((values) => {
          let errors = values.filter((value) => value.error)
          console.log(errors)
          if (errors.length > 0) {
            setErrors(values)
          } else {
            setErrors(undefined)
          }
        })
      } catch (e) {
        console.log(e)
        setErrors(undefined)
      }
    }
  }, [fakts])

  if (errors) {
    return Fallback ? (
      <>
        (<Fallback errors={errors} />) as React.ReactNode)
      </>
    ) : (
      <>'Error on services'</>
    )
  }

  if (fakts) return <>{children}</>

  return <>Did not find fakts!</>
}
