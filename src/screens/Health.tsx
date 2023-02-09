import React, { useEffect, useState } from 'react'
import { ResponsiveGrid } from '../layout/ResponsiveGrid'
import { useFakts } from '../fakts/fakts-config'

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

export const ServiceHealth = (props: { service: any }) => {
  return (
    <ResponsiveGrid>
      {props?.service?.ok &&
        Object.keys(props?.service?.ok).map((key) => (
          <div className='border rounded bg-green-200 border-green-600  p-3 shadow-xl'>
            <div className='font-light'>{key}</div>
            {JSON.stringify((props?.service?.ok as any)[key])}
          </div>
        ))}
      {props?.service?.error &&
        Object.keys(props?.service?.error).map((key) => (
          <div className='border rounded bg-red-200 border-red-600  p-3 shadow-xl'>
            <div className='font-light'>{key}</div>
            {JSON.stringify((props?.service?.error as any)[key])}
          </div>
        ))}
    </ResponsiveGrid>
  )
}

export const Health: React.FC<{}> = ({}) => {
  const { fakts } = useFakts()
  const [error, setError] = useState<string | undefined>()
  const [health, setHealth] = useState<HealthReturn[] | undefined>()

  useEffect(() => {
    if (fakts) {
      try {
        let checks = []

        for (let key in fakts) {
          let value = fakts[key as keyof any] as any
          if (value.healthz) {
            checks.push(checkHealth(key, value.healthz))
          }
        }

        Promise.all(checks).then((values) => {
          let errors = values.filter((value) => value.error)
          console.log(errors)
          setHealth(values)
        })
      } catch (e) {
        console.log(e)
        setError('Couldnt fetch health')
      }
    }
  }, [fakts])

  if (!fakts) return <>No fakts</>

  if (error) {
    return <>{error}</>
  }

  return (
    <>
      {health?.map((service) => (
        <>
          <div className='font-light mt-2'>{service.name} Status</div>
          <ServiceHealth service={service} />
        </>
      ))}
    </>
  )
}
